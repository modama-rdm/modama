use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use futures_util::{SinkExt as _, StreamExt as _};
use modama_common::{
    foreground::ScopeView,
    internal_api_types::{ToAgentFromGui, ToSessionGui},
};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

use crate::{
    helper_launch::HelperLauncher,
    state::{ActiveSession, AgentView, ForegroundSession},
};

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
    views: mpsc::Receiver<AgentView>,
    force_release: tokio_mpsc::UnboundedSender<()>,
    stop: Option<oneshot::Sender<()>>,
}

/// Sends the app each changed agent view, in order, so it sees every state, however brief
struct Views {
    tx: mpsc::Sender<AgentView>,
    last: AgentView,
    ctx: egui::Context,
}

impl Views {
    /// Send the view if it changed; false once the app is gone
    fn publish(&mut self, view: AgentView) -> bool {
        if view == self.last {
            return true;
        }
        self.last = view.clone();
        let sent = self.tx.send(view).is_ok();
        self.ctx.request_repaint();
        sent
    }

    /// Keep showing what the agent reported last, without the release offer: a release can't
    /// be requested without a connection
    fn publish_dropped(&mut self) -> bool {
        self.publish(AgentView {
            hand_off_blocked: false,
            connection_dropped: true,
            ..self.last.clone()
        })
    }
}

impl Drop for Views {
    /// However the monitor stops, the app sees the connection as dropped
    fn drop(&mut self) {
        self.publish_dropped();
    }
}

struct MonitorState {
    /// Only Windows mappings block each other, so only there is a release offered
    offer_force_release: bool,
    last_launch: Option<Instant>,
    launcher: Box<dyn HelperLauncher>,
}

impl MonitorState {
    fn on_message(&mut self, message: &ToSessionGui, views: &mut Views) -> bool {
        if message.session_helper.is_none()
            && self
                .last_launch
                .is_none_or(|launched| launched.elapsed() >= RELAUNCH_INTERVAL)
        {
            self.last_launch = Some(Instant::now());
            self.launcher.launch();
        }

        let view = ScopeView::new(
            message.desired_shares.as_ref(),
            message.session_helper.as_ref(),
        );
        let foreground = match view.foreground() {
            Ok(foreground) => ForegroundSession::from(foreground),
            Err(e) => {
                log::warn!("ignoring foreground ingest share: {e}");
                views.last.foreground.clone()
            }
        };
        views.publish(AgentView {
            foreground,
            other_mounts: view
                .other_mounts()
                .into_iter()
                .map(ActiveSession::from)
                .collect(),
            connected_agents: message.connected_agents.clone(),
            hand_off_blocked: self.offer_force_release && view.hand_off_blocked(),
            connection_dropped: false,
        })
    }
}

/// Follow the agent's stream until the app is gone, reconnecting as needed
async fn run(
    events_url: Url,
    mut state: MonitorState,
    mut views: Views,
    mut force_release: tokio_mpsc::UnboundedReceiver<()>,
    reconnect_delay: Duration,
) {
    loop {
        match tokio::time::timeout(CONNECT_TIMEOUT, connect_async(events_url.as_str())).await {
            Ok(Ok((mut socket, _response))) => {
                // requests are one-shot: those from while we were disconnected are dropped
                while force_release.try_recv().is_ok() {}
                loop {
                    tokio::select! {
                        message = socket.next() => match message {
                            Some(Ok(Message::Text(text))) => {
                                match serde_json::from_str::<ToSessionGui>(text.as_str()) {
                                    Ok(message) => {
                                        if !state.on_message(&message, &mut views) {
                                            return;
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!("ignoring invalid message from the agent: {e}")
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => break,
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                log::info!("connection to the agent failed ({e})");
                                break;
                            }
                        },
                        Some(()) = force_release.recv() => {
                            let text = match serde_json::to_string(&ToAgentFromGui::ForceRelease) {
                                Ok(text) => text,
                                Err(e) => {
                                    log::error!("could not serialize the release request: {e}");
                                    continue;
                                }
                            };
                            if let Err(e) = socket.send(Message::Text(text.into())).await {
                                log::info!("connection to the agent failed ({e})");
                                break;
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => log::warn!("could not connect to the agent: {e}"),
            Err(_elapsed) => log::warn!("could not connect to the agent: timed out"),
        }
        if !views.publish_dropped() {
            return;
        }
        tokio::time::sleep(reconnect_delay).await;
    }
}

impl ForegroundMonitor {
    /// Follow the agent, starting from the view the app shows now
    pub fn start(
        events_url: Url,
        launcher: Box<dyn HelperLauncher>,
        initial: AgentView,
        ctx: egui::Context,
    ) -> Self {
        Self::start_with(
            events_url,
            launcher,
            initial,
            ctx,
            RECONNECT_DELAY,
            cfg!(windows),
        )
    }

    fn start_with(
        events_url: Url,
        launcher: Box<dyn HelperLauncher>,
        initial: AgentView,
        ctx: egui::Context,
        reconnect_delay: Duration,
        offer_force_release: bool,
    ) -> Self {
        let (tx, views_rx) = mpsc::channel();
        let views = Views {
            tx,
            last: initial,
            ctx,
        };
        let (force_release, force_release_rx) = tokio_mpsc::unbounded_channel();
        let (stop, stop_rx) = oneshot::channel();
        let state = MonitorState {
            offer_force_release,
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
                    // returning drops `views`, which shows the connection as dropped
                    log::error!("could not start the agent monitor: {e}");
                    return;
                }
            };
            runtime.block_on(async move {
                tokio::select! {
                    () = run(
                        events_url,
                        state,
                        views,
                        force_release_rx,
                        reconnect_delay,
                    ) => {}
                    _ = stop_rx => {}
                }
            });
        });
        Self {
            views: views_rx,
            force_release,
            stop: Some(stop),
        }
    }

    /// Ask the agent to force the release of the previous user's drive; dropped if we aren't
    /// connected right now
    pub fn force_release(&self) {
        let _ = self.force_release.send(());
    }

    /// The next agent view; disconnected once the monitor stopped, which it does only if it
    /// could not start or failed. Its last view shows the connection as dropped.
    pub fn try_recv(&self) -> Result<AgentView, mpsc::TryRecvError> {
        self.views.try_recv()
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

    use modama_common::{
        internal_api_types::{
            ConnectedAgent, ConnectedAgents, ConnectionDetails, MountedShare, SessionHelperState,
            ShareError, Shares,
        },
        messages::{ConnectionId, Share},
    };
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::{WebSocketStream, accept_async};

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
            let monitor = ForegroundMonitor::start_with(
                Url::parse(&format!("ws://{address}/")).unwrap(),
                Box::new(launcher.clone()),
                AgentView::default(),
                egui::Context::default(),
                Duration::from_millis(10),
                true,
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

        /// The next published view, or None if none arrives within `wait`
        async fn next_view(&self, wait: Duration) -> Option<AgentView> {
            let deadline = Instant::now() + wait;
            loop {
                match self.monitor.try_recv() {
                    Ok(view) => return Some(view),
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
            created_by: None,
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
                reconciled_share_ids: None,
            }),
            desired_shares: Some(Shares {
                foreground_share_id: Some(id.to_owned()),
                shares: vec![share(id)],
            }),
            connected_agents: None,
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

    fn session(id: &str) -> ActiveSession {
        ActiveSession {
            share_id: id.to_owned(),
            project: "project".to_owned(),
            session: id.to_owned(),
            mount_path: Some(format!("/mnt/{id}")),
            created_by: None,
        }
    }

    fn foreground(id: &str) -> AgentView {
        AgentView {
            foreground: ForegroundSession::Mounted(session(id)),
            ..AgentView::default()
        }
    }

    #[tokio::test]
    async fn a_changed_view_is_published_once() {
        let setup = Setup::start().await;
        let mut agent = setup.accept().await;

        send(&mut agent, &mounted("a")).await;
        assert_eq!(setup.next_view(LONG).await, Some(foreground("a")));

        send(&mut agent, &mounted("a")).await;
        assert_eq!(setup.next_view(SHORT).await, None);
        assert_eq!(setup.launcher.count(), 0);
    }

    /// `old` and `new` both mounted, `new` in the foreground
    fn two_mounted() -> ToSessionGui {
        ToSessionGui {
            session_helper: Some(SessionHelperState {
                mounted_shares: ["old", "new"]
                    .map(|id| MountedShare {
                        share_name: format!("share-{id}"),
                        mount_path: Some(format!("/mnt/{id}").into()),
                    })
                    .into(),
                errors: vec![],
                reconciled_share_ids: None,
            }),
            desired_shares: Some(Shares {
                foreground_share_id: Some("new".to_owned()),
                shares: vec![share("old"), share("new")],
            }),
            connected_agents: None,
        }
    }

    #[tokio::test]
    async fn a_share_leaving_the_foreground_is_another_mount_in_the_same_view() {
        let setup = Setup::start().await;
        let mut agent = setup.accept().await;

        send(&mut agent, &mounted("old")).await;
        assert_eq!(setup.next_view(LONG).await, Some(foreground("old")));

        // a new share takes the foreground; the old one stays mounted next to it
        send(&mut agent, &two_mounted()).await;
        assert_eq!(
            setup.next_view(LONG).await,
            Some(AgentView {
                other_mounts: vec![session("old")],
                ..foreground("new")
            })
        );

        send(&mut agent, &two_mounted()).await;
        assert_eq!(setup.next_view(SHORT).await, None);
    }

    fn connection(id: u8) -> ConnectionDetails {
        ConnectionDetails {
            id: ConnectionId(id.into()),
            peer: std::net::SocketAddr::from(([192, 0, 2, id], 50000)),
            lender: None,
        }
    }

    #[tokio::test]
    async fn connected_agents_are_published_once_per_change() {
        let setup = Setup::start().await;
        let mut agent = setup.accept().await;

        send(&mut agent, &mounted("a")).await;
        assert_eq!(setup.next_view(LONG).await, Some(foreground("a")));

        let agents = ConnectedAgents {
            resource: "Holo".to_owned(),
            own: ConnectedAgent {
                name: "k2".to_owned(),
                verified: true,
                status: None,
                connection: connection(1),
            },
            others: vec![ConnectedAgent {
                name: "detector".to_owned(),
                verified: true,
                status: None,
                connection: connection(2),
            }],
        };
        let mut with_agents = mounted("a");
        with_agents.connected_agents = Some(agents.clone());
        send(&mut agent, &with_agents).await;
        assert_eq!(
            setup.next_view(LONG).await,
            Some(AgentView {
                connected_agents: Some(agents),
                ..foreground("a")
            })
        );

        send(&mut agent, &with_agents).await;
        assert_eq!(setup.next_view(SHORT).await, None);
    }

    #[tokio::test]
    async fn dropped_connection_is_published_once_and_recovers() {
        let setup = Setup::start().await;
        let agent = setup.accept().await;
        drop(agent);

        let dropped = AgentView {
            connection_dropped: true,
            ..AgentView::default()
        };
        assert_eq!(setup.next_view(LONG).await, Some(dropped));
        // the reconnect attempt fails as well, without another view:
        let agent = setup.accept().await;
        drop(agent);
        assert_eq!(setup.next_view(SHORT).await, None);

        let mut agent = setup.accept().await;
        send(&mut agent, &mounted("a")).await;
        assert_eq!(setup.next_view(LONG).await, Some(foreground("a")));
    }

    /// The previous user's `old` is still mounted, which blocks `new`
    fn blocked() -> ToSessionGui {
        let mut blocked = mounted("old");
        blocked.desired_shares = Some(Shares {
            foreground_share_id: Some("new".to_owned()),
            shares: vec![share("new")],
        });
        if let Some(helper) = &mut blocked.session_helper {
            helper.errors.push(ShareError {
                share_id: "new".to_owned(),
                message: "another user's ingest share is still connected".to_owned(),
            });
        }
        blocked
    }

    #[tokio::test]
    async fn blocked_hand_off_is_published_and_release_is_sent() {
        let setup = Setup::start().await;
        let mut agent = setup.accept().await;

        send(&mut agent, &blocked()).await;
        assert!(setup.next_view(LONG).await.unwrap().hand_off_blocked);

        setup.monitor.force_release();
        let message = tokio::time::timeout(LONG, agent.next())
            .await
            .expect("no release request")
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<ToAgentFromGui>(message.to_text().unwrap()).unwrap(),
            ToAgentFromGui::ForceRelease
        );
    }

    #[tokio::test]
    async fn a_dropped_connection_keeps_the_shares_but_not_the_release_offer() {
        let setup = Setup::start().await;
        let mut agent = setup.accept().await;
        send(&mut agent, &blocked()).await;
        let blocked = setup.next_view(LONG).await.unwrap();
        assert!(blocked.hand_off_blocked);

        drop(agent);
        assert_eq!(
            setup.next_view(LONG).await,
            Some(AgentView {
                hand_off_blocked: false,
                connection_dropped: true,
                ..blocked
            })
        );
    }

    /// The monitor stops on its own only if its runtime can't be built or it panics, which a
    /// test can't cause; so this drops the `Views` the monitor would drop
    #[test]
    fn a_stopped_monitor_leaves_the_connection_dropped() {
        let (tx, rx) = mpsc::channel();
        drop(Views {
            tx,
            last: foreground("a"),
            ctx: egui::Context::default(),
        });
        assert_eq!(
            rx.try_recv(),
            Ok(AgentView {
                connection_dropped: true,
                ..foreground("a")
            })
        );
        assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[tokio::test]
    async fn missing_helper_is_launched_at_most_once_per_interval() {
        let setup = Setup::start().await;
        let mut agent = setup.accept().await;
        let no_helper = ToSessionGui {
            session_helper: None,
            desired_shares: None,
            connected_agents: None,
        };

        send(&mut agent, &no_helper).await;
        send(&mut agent, &no_helper).await;
        send(&mut agent, &mounted("a")).await;
        // the last message changes the foreground, so all three have been processed then:
        assert_eq!(setup.next_view(LONG).await, Some(foreground("a")));
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
