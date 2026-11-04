use std::{future::Future, pin::Pin, time::Duration};

use futures_util::{
    SinkExt as _, StreamExt as _,
    stream::{SplitSink, SplitStream},
};
use log::{debug, error, info, warn};
use modama_common::{
    foreground::{ScopeView, ShareMount, desired_share_ids, project_and_session, same_share},
    internal_api_types::{SessionHelperState, Shares, ToAgent, ToSessionHelper},
    mount_popup::PopupMount,
};
use rand::RngExt;
use tokio::{
    net::TcpStream,
    sync::{mpsc, watch},
    time::Instant,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tungstenite::Message;
use url::Url;

use crate::{
    mount_backend::{MountBackend, PassResult},
    mount_popup::{CreatedMount, PopupEvent},
    tray::{TrayState, tray_state},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct LoopOptions {
    /// The agent's session helper endpoint, `ws://…/internal/events/session-helper/`
    pub events_url: Url,

    /// Time between reconciliation passes; each wait is drawn from ±20% of this, so helpers on
    /// many machines don't retry against the SMB host in lockstep
    pub interval: Duration,

    /// Wait this long before trying to reconnect to the agent
    pub reconnect_delay: Duration,
}

impl LoopOptions {
    pub fn new(events_url: Url) -> Self {
        Self {
            events_url,
            interval: Duration::from_secs(30),
            reconnect_delay: Duration::from_secs(1),
        }
    }

    fn next_pass(&self) -> Instant {
        Instant::now() + self.interval.mul_f64(rand::rng().random_range(0.8..=1.2))
    }
}

/// Everything the loop knows; owned by the single loop task, so passes can't overlap
struct LoopState {
    /// The last desired state received from the agent; kept across reconnects, so we keep
    /// reconciling while the agent is gone
    desired: Option<ToSessionHelper>,

    sink: Option<SplitSink<Socket, Message>>,
    stream: Option<SplitStream<Socket>>,

    /// What we last sent to the agent over the current connection
    last_report: Option<SessionHelperState>,

    /// Mounts a pass created, and which shares are still desired, go to the mount popup
    popup: mpsc::UnboundedSender<PopupEvent>,

    /// The desired share IDs the popup last heard of
    popup_desired: Option<Vec<String>>,

    /// The last pass's result, connected or not; for the notification-area icon
    actual: Option<SessionHelperState>,
    tray: watch::Sender<TrayState>,
}

/// The mounts this pass created that offer a path, as the mount popup shows them
fn popup_mounts(desired: &Shares, result: &PassResult) -> Vec<CreatedMount> {
    let view = ScopeView::new(Some(desired), Some(&result.state));
    result
        .created
        .iter()
        .filter_map(|name| {
            // the first desired share of that name is the one the pass mounted
            let (share, mount) = view
                .desired()
                .find(|(share, _)| same_share(&share.share_name, name))?;
            let ShareMount::Mounted(mount_path) = mount else {
                return None;
            };
            match project_and_session(&share.dest_path) {
                Ok((project, session)) => Some(CreatedMount {
                    share_id: share.share_id.clone(),
                    mount: PopupMount {
                        project,
                        session,
                        mount_path: mount_path.to_owned(),
                    },
                }),
                Err(e) => {
                    warn!("not announcing share {name}: {e}");
                    None
                }
            }
        })
        .collect()
}

impl LoopState {
    fn disconnected(&mut self) {
        self.sink = None;
        self.stream = None;
        self.last_report = None;
        self.publish_tray_state();
    }

    /// Tell the notification-area icon, if what it shows changed
    fn publish_tray_state(&self) {
        let state = tray_state(
            self.sink.is_some(),
            self.desired.as_ref(),
            self.actual.as_ref(),
        );
        self.tray.send_if_modified(|current| {
            let changed = *current != state;
            *current = state;
            changed
        });
    }

    /// One reconciliation pass, then report the result if it changed
    async fn pass(&mut self, backend: &impl MountBackend, force: bool) {
        let Some(desired) = &self.desired else {
            return;
        };
        // None: the agent hasn't heard from smbdrop yet, so we don't know what should be
        // mounted; leave everything alone
        let Some(share_state) = &desired.share_state else {
            return;
        };
        // before this pass's mounts, so the popup never shows a share that is gone already;
        // the popup task only stops with the helper
        let desired_ids = desired_share_ids(share_state);
        if self.popup_desired.as_ref() != Some(&desired_ids) {
            let _ = self.popup.send(PopupEvent::Desired(desired_ids.clone()));
            self.popup_desired = Some(desired_ids);
        }

        let result = match backend
            .reconcile(&share_state.shares, &desired.config, force)
            .await
        {
            Ok(result) => result,
            Err(e) => {
                // we don't know the actual state, so we don't claim anything
                warn!("{e}");
                return;
            }
        };
        let created = popup_mounts(share_state, &result);
        if !created.is_empty() {
            info!("created the following shares: {created:?}");
            let _ = self.popup.send(PopupEvent::Created(created));
        }
        // so the agent can tell which desired state this report answers
        let report = SessionHelperState {
            reconciled_share_ids: Some(desired_share_ids(share_state)),
            ..result.state
        };
        self.actual = Some(report.clone());
        self.publish_tray_state();

        let Some(sink) = &mut self.sink else {
            return;
        };
        if self.last_report.as_ref() == Some(&report) {
            return;
        }
        let msg = ToAgent::SessionUpdate {
            shares: report.mounted_shares.clone(),
            errors: report.errors.clone(),
            reconciled_share_ids: report.reconciled_share_ids.clone(),
        };
        let text = match serde_json::to_string(&msg) {
            Ok(text) => text,
            Err(e) => {
                error!("could not serialize report: {e}");
                return;
            }
        };
        match sink.send(Message::Text(text.into())).await {
            Ok(()) => self.last_report = Some(report),
            Err(e) => {
                info!("could not send report ({e}), reconnecting...");
                self.disconnected();
            }
        }
    }
}

/// The agent must answer the websocket handshake within this time
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

type Connecting = Pin<Box<dyn Future<Output = Result<Socket, String>> + Send>>;

fn connect(url: Url) -> Connecting {
    Box::pin(async move {
        match tokio::time::timeout(CONNECT_TIMEOUT, connect_async(url.as_str())).await {
            Ok(Ok((socket, _response))) => Ok(socket),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_elapsed) => Err("timed out".to_owned()),
        }
    })
}

/// Wait for the connection attempt, or forever if there is none
async fn connected(connecting: &mut Option<Connecting>) -> Result<Socket, String> {
    match connecting {
        Some(connecting) => connecting.await,
        None => std::future::pending().await,
    }
}

/// Wait for the next message, or forever if we aren't connected
async fn next_message(
    stream: &mut Option<SplitStream<Socket>>,
) -> Option<Result<Message, tungstenite::Error>> {
    match stream {
        Some(stream) => stream.next().await,
        None => std::future::pending().await,
    }
}

/// Connect to the agent, receive desired state, reconcile and report, forever.
pub async fn session_helper_loop(
    backend: &impl MountBackend,
    options: &LoopOptions,
    popup: mpsc::UnboundedSender<PopupEvent>,
    tray: watch::Sender<TrayState>,
) -> ! {
    let mut state = LoopState {
        desired: None,
        sink: None,
        stream: None,
        last_report: None,
        popup,
        popup_desired: None,
        actual: None,
        tray,
    };
    state.publish_tray_state();
    let mut next_pass = options.next_pass();
    let mut next_connect = Instant::now();
    // connecting runs alongside the passes, so an agent that hangs during the handshake doesn't
    // stop reconciliation
    let mut connecting: Option<Connecting> = None;

    loop {
        let idle = state.stream.is_none() && connecting.is_none();
        tokio::select! {
            message = next_message(&mut state.stream) => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ToSessionHelper>(text.as_str()) {
                            Ok(desired) => {
                                debug!("received desired state: {desired:?}");
                                // forcing applies to this one pass only, not to later ones
                                let force = desired.force_release_obsolete;
                                state.desired = Some(ToSessionHelper {
                                    force_release_obsolete: false,
                                    ..desired
                                });
                                state.pass(backend, force).await;
                                next_pass = options.next_pass();
                            }
                            Err(e) => warn!("ignoring invalid message from the agent: {e}"),
                        }
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                    Some(Ok(Message::Binary(_))) => warn!("ignoring binary message from the agent"),
                    Some(Ok(Message::Close(_))) | None => {
                        info!("disconnected from the agent, reconnecting...");
                        state.disconnected();
                        next_connect = Instant::now() + options.reconnect_delay;
                    }
                    Some(Err(e)) => {
                        info!("connection to the agent failed ({e}), reconnecting...");
                        state.disconnected();
                        next_connect = Instant::now() + options.reconnect_delay;
                    }
                }
            }
            _ = tokio::time::sleep_until(next_pass) => {
                state.pass(backend, false).await;
                next_pass = options.next_pass();
            }
            _ = tokio::time::sleep_until(next_connect), if idle => {
                connecting = Some(connect(options.events_url.clone()));
            }
            result = connected(&mut connecting) => {
                connecting = None;
                match result {
                    Ok(socket) => {
                        info!("connected to the agent at {}", options.events_url);
                        let (sink, stream) = socket.split();
                        state.sink = Some(sink);
                        state.stream = Some(stream);
                        state.publish_tray_state();
                    }
                    Err(e) => {
                        debug!("could not connect to the agent: {e}");
                        next_connect = Instant::now() + options.reconnect_delay;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use modama_common::{
        internal_api_types::{MountedShare, SessionHelperConfig, ShareError, Shares},
        messages::Share,
    };
    use tokio::{net::TcpListener, task::JoinHandle, time::timeout};
    use tokio_tungstenite::accept_async;

    use super::*;
    use crate::mount_backend::EnumerationError;

    type AgentSocket = WebSocketStream<TcpStream>;

    /// Records the share IDs of every pass, and returns whatever result is configured
    #[derive(Clone)]
    struct FakeBackend {
        calls: Arc<Mutex<Vec<Vec<String>>>>,
        /// whether each pass was forced
        forced: Arc<Mutex<Vec<bool>>>,
        result: Arc<Mutex<Result<SessionHelperState, String>>>,
        /// returned as `created` by the next pass only
        created: Arc<Mutex<Vec<String>>>,
    }

    impl FakeBackend {
        fn new(result: Result<SessionHelperState, String>) -> Self {
            Self {
                calls: Arc::default(),
                forced: Arc::default(),
                result: Arc::new(Mutex::new(result)),
                created: Arc::default(),
            }
        }

        fn calls(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        fn set_result(&self, result: Result<SessionHelperState, String>) {
            *self.result.lock().unwrap() = result;
        }
    }

    impl MountBackend for FakeBackend {
        async fn reconcile(
            &self,
            desired_shares: &[Share],
            _config: &SessionHelperConfig,
            force: bool,
        ) -> Result<PassResult, EnumerationError> {
            self.forced.lock().unwrap().push(force);
            self.calls
                .lock()
                .unwrap()
                .push(desired_shares.iter().map(|s| s.share_id.clone()).collect());
            let state = self
                .result
                .lock()
                .unwrap()
                .clone()
                .map_err(EnumerationError)?;
            Ok(PassResult {
                state,
                created: std::mem::take(&mut *self.created.lock().unwrap()),
            })
        }
    }

    /// A fake agent: the helper connects to `listener`
    struct Setup {
        listener: TcpListener,
        backend: FakeBackend,
        helper: JoinHandle<()>,
        popup: mpsc::UnboundedReceiver<PopupEvent>,
        tray: watch::Receiver<TrayState>,
    }

    impl Setup {
        async fn start(result: Result<SessionHelperState, String>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let backend = FakeBackend::new(result);
            let options = LoopOptions {
                events_url: Url::parse(&format!("ws://{address}/")).unwrap(),
                interval: Duration::from_millis(30),
                reconnect_delay: Duration::from_millis(10),
            };
            let helper_backend = backend.clone();
            let (popup_tx, popup) = mpsc::unbounded_channel();
            let (tray_tx, tray) = watch::channel(TrayState::default());
            let helper = tokio::spawn(async move {
                session_helper_loop(&helper_backend, &options, popup_tx, tray_tx).await;
            });
            Self {
                listener,
                backend,
                helper,
                popup,
                tray,
            }
        }

        async fn accept(&self) -> AgentSocket {
            let (tcp, _) = timeout(Duration::from_secs(2), self.listener.accept())
                .await
                .expect("the helper did not connect")
                .unwrap();
            accept_async(tcp).await.unwrap()
        }

        /// Wait until the backend has seen at least `n` passes in total
        async fn wait_for_calls(&self, n: usize) {
            timeout(Duration::from_secs(2), async {
                while self.backend.calls() < n {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("not enough reconciliation passes");
        }
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            self.helper.abort();
        }
    }

    fn share(id: &str) -> Share {
        Share {
            resource: Some("resource".to_owned()),
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

    fn report(paths: &[&str]) -> SessionHelperState {
        SessionHelperState {
            mounted_shares: paths
                .iter()
                .map(|p| MountedShare {
                    share_name: format!("share-{p}"),
                    mount_path: Some(format!("/mnt/{p}").into()),
                })
                .collect(),
            errors: vec![],
            reconciled_share_ids: None,
        }
    }

    /// `state` as reported after a pass for the shares with the given IDs
    fn echo(state: SessionHelperState, ids: &[&str]) -> SessionHelperState {
        SessionHelperState {
            reconciled_share_ids: Some(ids.iter().map(|id| (*id).to_owned()).collect()),
            ..state
        }
    }

    async fn send_desired(socket: &mut AgentSocket, shares: Option<Vec<Share>>) {
        send_to_helper(socket, shares, false).await;
    }

    async fn send_to_helper(
        socket: &mut AgentSocket,
        shares: Option<Vec<Share>>,
        force_release_obsolete: bool,
    ) {
        let msg = ToSessionHelper {
            config: SessionHelperConfig {
                smb_hostname: "smb.example.test".to_owned(),
                smb_port: 445,
                smb_domain: "WORKGROUP".to_owned(),
            },
            share_state: shares.map(|shares| Shares {
                foreground_share_id: None,
                shares,
            }),
            force_release_obsolete,
        };
        socket
            .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
            .await
            .unwrap();
    }

    /// The next report, or None if none arrives within `wait`
    async fn next_report(socket: &mut AgentSocket, wait: Duration) -> Option<SessionHelperState> {
        let message = timeout(wait, socket.next()).await.ok()?;
        let text = message.unwrap().unwrap().into_text().unwrap();
        let ToAgent::SessionUpdate {
            shares,
            errors,
            reconciled_share_ids,
        } = serde_json::from_str(&text).unwrap();
        Some(SessionHelperState {
            mounted_shares: shares,
            errors,
            reconciled_share_ids,
        })
    }

    const SHORT: Duration = Duration::from_millis(200);
    const LONG: Duration = Duration::from_secs(2);

    #[tokio::test]
    async fn desired_state_triggers_a_pass_and_its_result_is_reported() {
        let mut result = report(&["a"]);
        result.errors.push(ShareError {
            share_id: "b".to_owned(),
            message: "credential conflict".to_owned(),
        });
        let setup = Setup::start(Ok(result.clone())).await;
        let mut agent = setup.accept().await;

        send_desired(&mut agent, Some(vec![share("a"), share("b")])).await;

        assert_eq!(
            next_report(&mut agent, LONG).await,
            Some(echo(result, &["a", "b"]))
        );
        assert_eq!(setup.backend.calls.lock().unwrap()[0], vec!["a", "b"]);
    }

    #[tokio::test]
    async fn forced_release_applies_to_one_pass_only() {
        let setup = Setup::start(Ok(report(&["a"]))).await;
        let mut agent = setup.accept().await;

        send_to_helper(&mut agent, Some(vec![share("a")]), true).await;

        setup.wait_for_calls(3).await;
        let forced = setup.backend.forced.lock().unwrap().clone();
        assert!(forced[0]);
        assert!(forced[1..].iter().all(|forced| !forced));
    }

    #[tokio::test]
    async fn created_mounts_with_a_path_go_to_the_popup() {
        let mut setup = Setup::start(Ok(report(&["a"]))).await;
        // "share-b" was mounted, but has no path in the report:
        *setup.backend.created.lock().unwrap() = vec!["share-a".to_owned(), "share-b".to_owned()];
        let mut agent = setup.accept().await;

        send_desired(&mut agent, Some(vec![share("a"), share("b")])).await;

        let first = timeout(LONG, setup.popup.recv()).await.unwrap().unwrap();
        assert_eq!(first, desired_ids(&["a", "b"]));
        let mounts = timeout(LONG, setup.popup.recv()).await.unwrap().unwrap();
        assert_eq!(
            mounts,
            PopupEvent::Created(vec![CreatedMount {
                share_id: "a".to_owned(),
                mount: PopupMount {
                    project: "project".to_owned(),
                    session: "a".to_owned(),
                    mount_path: "/mnt/a".into(),
                },
            }])
        );
        // later passes create nothing and the desired state stays, so nothing more is announced:
        setup.wait_for_calls(3).await;
        assert!(setup.popup.try_recv().is_err());
    }

    fn desired_ids(ids: &[&str]) -> PopupEvent {
        PopupEvent::Desired(ids.iter().map(|id| (*id).to_owned()).collect())
    }

    #[tokio::test]
    async fn the_popup_hears_when_the_desired_shares_change_but_not_while_unknown() {
        let mut setup = Setup::start(Ok(report(&[]))).await;
        let mut agent = setup.accept().await;

        // the agent doesn't know yet what should be mounted: nothing is desired or undesired
        send_desired(&mut agent, None).await;
        send_desired(&mut agent, Some(vec![share("a"), share("b")])).await;
        let event = timeout(LONG, setup.popup.recv()).await.unwrap().unwrap();
        assert_eq!(event, desired_ids(&["a", "b"]));

        send_desired(&mut agent, Some(vec![share("b")])).await;
        let event = timeout(LONG, setup.popup.recv()).await.unwrap().unwrap();
        assert_eq!(event, desired_ids(&["b"]));

        send_desired(&mut agent, Some(vec![])).await;
        let event = timeout(LONG, setup.popup.recv()).await.unwrap().unwrap();
        assert_eq!(event, desired_ids(&[]));
        setup.wait_for_calls(4).await;
        assert!(setup.popup.try_recv().is_err());
    }

    #[tokio::test]
    async fn the_tray_follows_connection_and_convergence() {
        use crate::tray::TrayStatus;

        let mut setup = Setup::start(Ok(report(&["a"]))).await;
        assert_eq!(setup.tray.borrow().status, TrayStatus::Disconnected);
        let mut agent = setup.accept().await;

        send_desired(&mut agent, Some(vec![share("a")])).await;
        timeout(
            LONG,
            setup
                .tray
                .wait_for(|state| state.status == TrayStatus::Ready),
        )
        .await
        .unwrap()
        .unwrap();

        drop(agent);
        timeout(
            LONG,
            setup
                .tray
                .wait_for(|state| state.status == TrayStatus::Disconnected),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test]
    async fn no_pass_before_the_agent_knows_the_desired_state() {
        let setup = Setup::start(Ok(report(&[]))).await;
        let mut agent = setup.accept().await;

        send_desired(&mut agent, None).await;

        // several intervals pass without any reconciliation or report:
        assert_eq!(next_report(&mut agent, SHORT).await, None);
        assert_eq!(setup.backend.calls(), 0);
    }

    #[tokio::test]
    async fn unchanged_reports_are_sent_once() {
        let setup = Setup::start(Ok(report(&["a"]))).await;
        let mut agent = setup.accept().await;
        send_desired(&mut agent, Some(vec![share("a")])).await;
        assert_eq!(
            next_report(&mut agent, LONG).await,
            Some(echo(report(&["a"]), &["a"]))
        );

        setup.wait_for_calls(4).await;
        assert_eq!(next_report(&mut agent, SHORT).await, None);

        setup.backend.set_result(Ok(report(&[])));
        assert_eq!(
            next_report(&mut agent, LONG).await,
            Some(echo(report(&[]), &["a"]))
        );
    }

    #[tokio::test]
    async fn enumeration_errors_are_not_reported() {
        let setup = Setup::start(Err("no GVfs".to_owned())).await;
        let mut agent = setup.accept().await;

        send_desired(&mut agent, Some(vec![share("a")])).await;

        setup.wait_for_calls(2).await;
        assert_eq!(next_report(&mut agent, SHORT).await, None);
    }

    #[tokio::test]
    async fn reconciles_while_disconnected_and_reports_again_after_reconnect() {
        let setup = Setup::start(Ok(report(&["a"]))).await;
        let mut agent = setup.accept().await;
        send_desired(&mut agent, Some(vec![share("a")])).await;
        assert_eq!(
            next_report(&mut agent, LONG).await,
            Some(echo(report(&["a"]), &["a"]))
        );

        // the agent goes away; the helper keeps reconciling the retained desired state
        drop(agent);
        let calls = setup.backend.calls();
        setup.wait_for_calls(calls + 3).await;

        // after the reconnect, the same (unchanged) state is reported again
        let mut agent = setup.accept().await;
        send_desired(&mut agent, Some(vec![share("a")])).await;
        assert_eq!(
            next_report(&mut agent, LONG).await,
            Some(echo(report(&["a"]), &["a"]))
        );
    }

    #[tokio::test]
    async fn hanging_handshake_does_not_stop_reconciliation() {
        let setup = Setup::start(Ok(report(&["a"]))).await;
        let mut agent = setup.accept().await;
        send_desired(&mut agent, Some(vec![share("a")])).await;
        assert_eq!(
            next_report(&mut agent, LONG).await,
            Some(echo(report(&["a"]), &["a"]))
        );
        drop(agent);

        // accept the TCP connection, but never answer the websocket handshake
        let (_tcp, _) = timeout(LONG, setup.listener.accept())
            .await
            .unwrap()
            .unwrap();
        let calls = setup.backend.calls();
        setup.wait_for_calls(calls + 3).await;
    }

    /// A new desired state gets a report, even if the pass result is the same as before:
    /// otherwise the agent couldn't tell that the pass for it already ran
    #[tokio::test]
    async fn new_desired_state_is_reported_even_with_the_same_result() {
        let setup = Setup::start(Ok(report(&["a"]))).await;
        let mut agent = setup.accept().await;
        send_desired(&mut agent, Some(vec![share("a")])).await;
        assert_eq!(
            next_report(&mut agent, LONG).await,
            Some(echo(report(&["a"]), &["a"]))
        );

        send_desired(&mut agent, Some(vec![share("b"), share("a")])).await;
        assert_eq!(
            next_report(&mut agent, LONG).await,
            Some(echo(report(&["a"]), &["a", "b"]))
        );
    }
}
