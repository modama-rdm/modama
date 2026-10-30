use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use futures_util::StreamExt as _;
use modama_common::{
    foreground::{foreground_projection, foreground_share},
    internal_api_types::ToSessionGui,
};
use tokio::sync::oneshot;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

use crate::{helper_launch::HelperLauncher, state::ForegroundSession};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForegroundMonitorUpdate {
    ForegroundChanged(ForegroundSession),
    ConnectionDropped,
    ConnectionRestored,
}

/// The agent must answer the websocket handshake within this time
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Wait this long before trying to reconnect to the agent
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Launch a session helper at most this often while the agent reports none
const RELAUNCH_INTERVAL: Duration = Duration::from_secs(60);

/// The GUI endpoint of the agent's internal stream, derived from the agent's base URL
pub fn events_url(agent_url: &str) -> Result<Url, String> {
    let mut url = Url::parse(agent_url)
        .and_then(|url| url.join("internal/events/gui/"))
        .map_err(|e| format!("invalid agent URL {agent_url:?}: {e}"))?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => return Err(format!("unsupported agent URL scheme {other:?}")),
    };
    url.set_scheme(scheme)
        .map_err(|()| format!("could not derive the websocket URL from {agent_url}"))?;
    Ok(url)
}

pub struct ForegroundMonitor {
    updates: mpsc::Receiver<ForegroundMonitorUpdate>,
    stop: Option<oneshot::Sender<()>>,
}

/// Forwards updates to the app; false once the app is gone
struct Updates {
    tx: mpsc::Sender<ForegroundMonitorUpdate>,
    ctx: egui::Context,
}

impl Updates {
    fn send(&self, update: ForegroundMonitorUpdate) -> bool {
        let sent = self.tx.send(update).is_ok();
        self.ctx.request_repaint();
        sent
    }
}

struct MonitorState {
    last: ForegroundSession,
    connection_dropped: bool,
    last_launch: Option<Instant>,
    launcher: Box<dyn HelperLauncher>,
}

impl MonitorState {
    fn on_message(&mut self, message: &ToSessionGui, updates: &Updates) -> bool {
        if self.connection_dropped {
            self.connection_dropped = false;
            if !updates.send(ForegroundMonitorUpdate::ConnectionRestored) {
                return false;
            }
        }

        if message.session_helper.is_none()
            && self
                .last_launch
                .is_none_or(|launched| launched.elapsed() >= RELAUNCH_INTERVAL)
        {
            self.last_launch = Some(Instant::now());
            self.launcher.launch();
        }

        let desired = message.desired_shares.as_ref().and_then(foreground_share);
        let foreground = match foreground_projection(desired, message.session_helper.as_ref()) {
            Ok(foreground) => ForegroundSession::from(foreground),
            Err(e) => {
                log::warn!("ignoring foreground share: {e}");
                return true;
            }
        };
        if foreground != self.last {
            self.last = foreground.clone();
            return updates.send(ForegroundMonitorUpdate::ForegroundChanged(foreground));
        }
        true
    }

    fn on_disconnect(&mut self, updates: &Updates) -> bool {
        if self.connection_dropped {
            return true;
        }
        self.connection_dropped = true;
        updates.send(ForegroundMonitorUpdate::ConnectionDropped)
    }
}

/// Follow the agent's stream until the app is gone, reconnecting as needed
async fn run(
    events_url: Url,
    mut state: MonitorState,
    updates: Updates,
    reconnect_delay: Duration,
) {
    loop {
        match tokio::time::timeout(CONNECT_TIMEOUT, connect_async(events_url.as_str())).await {
            Ok(Ok((mut socket, _response))) => {
                while let Some(message) = socket.next().await {
                    match message {
                        Ok(Message::Text(text)) => {
                            match serde_json::from_str::<ToSessionGui>(text.as_str()) {
                                Ok(message) => {
                                    if !state.on_message(&message, &updates) {
                                        return;
                                    }
                                }
                                Err(e) => {
                                    log::warn!("ignoring invalid message from the agent: {e}")
                                }
                            }
                        }
                        Ok(Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(e) => {
                            log::info!("connection to the agent failed ({e})");
                            break;
                        }
                    }
                }
            }
            Ok(Err(e)) => log::warn!("could not connect to the agent: {e}"),
            Err(_elapsed) => log::warn!("could not connect to the agent: timed out"),
        }
        if !state.on_disconnect(&updates) {
            return;
        }
        tokio::time::sleep(reconnect_delay).await;
    }
}

impl ForegroundMonitor {
    pub fn start(
        events_url: Url,
        launcher: Box<dyn HelperLauncher>,
        initial: ForegroundSession,
        ctx: egui::Context,
    ) -> Self {
        Self::start_with_delay(events_url, launcher, initial, ctx, RECONNECT_DELAY)
    }

    fn start_with_delay(
        events_url: Url,
        launcher: Box<dyn HelperLauncher>,
        initial: ForegroundSession,
        ctx: egui::Context,
        reconnect_delay: Duration,
    ) -> Self {
        let (tx, updates) = mpsc::channel();
        let (stop, stop_rx) = oneshot::channel();
        let state = MonitorState {
            last: initial,
            connection_dropped: false,
            last_launch: None,
            launcher,
        };
        thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    // dropping `tx` makes the app treat the connection as dropped
                    log::error!("could not start the agent monitor: {e}");
                    return;
                }
            };
            runtime.block_on(async move {
                tokio::select! {
                    () = run(events_url, state, Updates { tx, ctx }, reconnect_delay) => {}
                    _ = stop_rx => {}
                }
            });
        });
        Self {
            updates,
            stop: Some(stop),
        }
    }

    pub fn try_recv(&self) -> Result<ForegroundMonitorUpdate, mpsc::TryRecvError> {
        self.updates.try_recv()
    }
}

impl Drop for ForegroundMonitor {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use futures_util::SinkExt as _;
    use modama_common::{
        internal_api_types::{MountedShare, SessionHelperState, Shares},
        messages::Share,
    };
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::{WebSocketStream, accept_async};

    use crate::state::ActiveSession;

    use super::*;

    #[derive(Clone, Default)]
    struct CountingLauncher(Arc<AtomicUsize>);

    impl HelperLauncher for CountingLauncher {
        fn launch(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl CountingLauncher {
        fn count(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    /// A fake agent; the monitor connects to `listener`
    struct Setup {
        listener: TcpListener,
        launcher: CountingLauncher,
        monitor: ForegroundMonitor,
    }

    impl Setup {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let launcher = CountingLauncher::default();
            let monitor = ForegroundMonitor::start_with_delay(
                Url::parse(&format!("ws://{address}/")).unwrap(),
                Box::new(launcher.clone()),
                ForegroundSession::None,
                egui::Context::default(),
                Duration::from_millis(10),
            );
            Self {
                listener,
                launcher,
                monitor,
            }
        }

        async fn accept(&self) -> WebSocketStream<TcpStream> {
            let (tcp, _) = tokio::time::timeout(LONG, self.listener.accept())
                .await
                .expect("the monitor did not connect")
                .unwrap();
            accept_async(tcp).await.unwrap()
        }

        /// The next update, or None if none arrives within `wait`
        async fn next_update(&self, wait: Duration) -> Option<ForegroundMonitorUpdate> {
            let deadline = Instant::now() + wait;
            loop {
                match self.monitor.try_recv() {
                    Ok(update) => return Some(update),
                    Err(mpsc::TryRecvError::Empty) if Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    Err(_) => return None,
                }
            }
        }
    }

    const SHORT: Duration = Duration::from_millis(200);
    const LONG: Duration = Duration::from_secs(2);

    fn share(id: &str) -> Share {
        Share {
            resource: None,
            share_id: id.to_owned(),
            share_name: format!("share-{id}"),
            dest_path: format!("/project/{id}"),
            created_timestamp: 0,
            username: "user".to_owned(),
            password: "password".to_owned(),
            uid: 1000,
            gid: 1000,
        }
    }

    fn mounted(id: &str) -> ToSessionGui {
        ToSessionGui {
            session_helper: Some(SessionHelperState {
                mounted_shares: vec![MountedShare {
                    share_name: format!("share-{id}"),
                    mount_path: Some(format!("/mnt/{id}").into()),
                }],
                errors: vec![],
            }),
            desired_shares: Some(Shares {
                foreground_share_id: Some(id.to_owned()),
                shares: vec![share(id)],
            }),
        }
    }

    async fn send(socket: &mut WebSocketStream<TcpStream>, message: &ToSessionGui) {
        socket
            .send(Message::Text(
                serde_json::to_string(message).unwrap().into(),
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn changed_foreground_is_forwarded_once() {
        let setup = Setup::start().await;
        let mut agent = setup.accept().await;

        send(&mut agent, &mounted("a")).await;
        assert_eq!(
            setup.next_update(LONG).await,
            Some(ForegroundMonitorUpdate::ForegroundChanged(
                ForegroundSession::Mounted(ActiveSession {
                    share_id: "a".to_owned(),
                    project: "project".to_owned(),
                    session: "a".to_owned(),
                    mount_path: Some("/mnt/a".to_owned()),
                })
            ))
        );

        send(&mut agent, &mounted("a")).await;
        assert_eq!(setup.next_update(SHORT).await, None);
        assert_eq!(setup.launcher.count(), 0);
    }

    #[tokio::test]
    async fn dropped_connection_is_reported_once_and_recovers() {
        let setup = Setup::start().await;
        let agent = setup.accept().await;
        drop(agent);

        assert_eq!(
            setup.next_update(LONG).await,
            Some(ForegroundMonitorUpdate::ConnectionDropped)
        );
        // the reconnect attempt fails as well, without another update:
        let agent = setup.accept().await;
        drop(agent);
        assert_eq!(setup.next_update(SHORT).await, None);

        let mut agent = setup.accept().await;
        send(&mut agent, &mounted("a")).await;
        assert_eq!(
            setup.next_update(LONG).await,
            Some(ForegroundMonitorUpdate::ConnectionRestored)
        );
    }

    #[tokio::test]
    async fn missing_helper_is_launched_at_most_once_per_interval() {
        let setup = Setup::start().await;
        let mut agent = setup.accept().await;
        let no_helper = ToSessionGui {
            session_helper: None,
            desired_shares: None,
        };

        send(&mut agent, &no_helper).await;
        send(&mut agent, &no_helper).await;
        send(&mut agent, &mounted("a")).await;
        // the last message changes the foreground, so all three have been processed then:
        assert!(matches!(
            setup.next_update(LONG).await,
            Some(ForegroundMonitorUpdate::ForegroundChanged(_))
        ));
        assert_eq!(setup.launcher.count(), 1);
    }

    #[test]
    fn events_url_is_derived_from_the_agent_url() {
        let url = |s: &str| events_url(s).map(|u| u.to_string());
        assert_eq!(
            url("http://localhost:3003").unwrap(),
            "ws://localhost:3003/internal/events/gui/"
        );
        assert_eq!(
            url("https://localhost:3003/").unwrap(),
            "wss://localhost:3003/internal/events/gui/"
        );
        assert!(url("ftp://localhost/").is_err());
    }
}
