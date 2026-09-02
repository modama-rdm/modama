use std::{ops::DerefMut as _, path::PathBuf, sync::Arc};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::header::HeaderMap,
    response::{IntoResponse as _, Response},
};
use hyper::StatusCode;
use log::{error, warn};
use modama_common::messages::Share;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    select,
    sync::{Mutex, broadcast, mpsc},
};

use crate::{
    agent_state::{AgentState, RemoteInfo, SessionShares, StateUpdateToSession},
    local_http_service::ApiState,
    share::MountedShare,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionId {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHelperConfig {
    smb_hostname: String,
    smb_port: u32,
}

#[derive(thiserror::Error, Debug)]
pub enum SessionError {
    #[error("could not determine session identity")]
    CouldNotId(#[from] std::env::VarError),
}

impl SessionId {
    pub fn from_env() -> Result<Self, SessionError> {
        #[cfg(target_os = "linux")]
        let environment = std::env::var("XDG_SESSION_ID")?;
        #[cfg(windows)]
        let environment = std::env::var("SESSIONNAME")?;

        Ok(Self { id: environment })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionShare {
    share: Share,
    path: PathBuf,
}

/// Messages sent over the internal websocket connection
/// from either modama-agent-gui or the session helper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InternalMsgToAgent {
    /// This must be the first message from a GUI connection
    HelloGui { session_id: SessionId },

    /// This must be the first message from a session helper connection
    HelloSessionHelper { session_id: SessionId },

    /// Update from the session helper informing about the currently mounted ingest shares
    SessionUpdate { shares: Vec<SessionShare> },
}

// TODO: figure out relation to the ShareState type - does that go away with the new design?
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shares {
    /// The foreground share ID is needed to have the notification tray UI present it
    foreground_share_id: Option<String>,

    /// All shares that should be mounted
    shares: Vec<Share>,
}

// TODO: maybe actually separate the *ToSession enum into two structs that carry the full state,
// which could simplify lagging and similar edge cases, but is it worth it?
/// Updates sent to the session helper and the GUI
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InternalMsgToSession {
    DesiredState {
        /// Configuration contains the samba hostname whose mounts are managed by the session helper
        /// (also the port if we are on Linux)
        config: SessionHelperConfig,

        /// None means we haven't received the desired state yet. We need to consider this, because
        /// otherwise, it could lead to a flip-flopping situation when re-connecting. If this is
        /// None, and shares are mounted, no reconciliation should be happening.
        share_state: Option<Shares>,
    },
    /// Updated state which sessions are mounted in the given desktop session
    DesktopSessionState {
        session_id: SessionId,
        mounted_shares: SessionShares,
        desired_shares: Option<Shares>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum EventAPIError {
    #[error("connection dropped")]
    Dropped,

    #[error("invalid message, dropping connection")]
    InvalidMsg,

    #[error("internal server error")]
    AxumError(#[from] axum::Error),

    #[error("serialization/message format error")]
    Serialization(#[from] serde_json::Error),

    #[error("internal communication error")]
    ChannelRecv(#[from] tokio::sync::broadcast::error::RecvError),

    #[error("internal communication error")]
    ChannelSend(#[from] tokio::sync::broadcast::error::SendError<StateUpdateToSession>),

    #[error("state error, message sent out of turn?")]
    StateError,
}

async fn recv_obj<T>(socket: &mut WebSocket) -> Result<T, EventAPIError>
where
    T: DeserializeOwned,
{
    loop {
        let msg = socket
            .recv()
            .await
            .transpose()?
            .ok_or(EventAPIError::Dropped)?;
        let msg_text = match &msg {
            Message::Text(utf8_bytes) => utf8_bytes.as_str(),
            Message::Ping(_a) | Message::Pong(_a) => continue,
            Message::Close(_close_frame) => return Err(EventAPIError::Dropped),
            _ => return Err(EventAPIError::InvalidMsg),
        };
        let msg_obj: T = serde_json::from_str(msg_text)?;

        return Ok(msg_obj);
    }
}

async fn send_obj<T>(socket: &mut WebSocket, obj: &T) -> Result<(), EventAPIError>
where
    T: Serialize,
{
    let msg = Message::Text(serde_json::to_string(obj)?.into());
    socket.send(msg).await?;

    Ok(())
}

async fn send_update_to_session_helper(
    update: StateUpdateToSession,
    config: &SessionHelperConfig,
    socket: &mut WebSocket,
) -> Result<(), EventAPIError> {
    match update {
        StateUpdateToSession::DesiredState {
            foreground_share_id,
            shares,
        } => {
            send_obj(
                socket,
                &InternalMsgToSession::DesiredState {
                    config: config.clone(),
                    share_state: Some(Shares {
                        foreground_share_id,
                        shares,
                    }),
                },
            )
            .await?;
        }
        StateUpdateToSession::MountUpdate {
            session_id: _,
            shares: _,
        } => {} // ignore
    }

    Ok(())
}

async fn record_update_from_session_helper(
    session_update: InternalMsgToAgent,
    tx: broadcast::Sender<StateUpdateToSession>,
    session_id: &SessionId,
    state: &mut AgentState,
) -> Result<(), EventAPIError> {
    match session_update {
        InternalMsgToAgent::HelloGui { session_id: _ }
        | InternalMsgToAgent::HelloSessionHelper { session_id: _ } => {
            Err(EventAPIError::StateError)
        }
        InternalMsgToAgent::SessionUpdate { shares } => {
            let state_mounted_shares = state
                .mounted_shares
                .entry(session_id.clone())
                .or_insert(SessionShares::NoSessionHelper);
            let mounted_shares: Vec<MountedShare> = shares
                .into_iter()
                .map(|s| MountedShare {
                    share: s.share,
                    mount_path: s.path,
                })
                .collect();
            *state_mounted_shares = SessionShares::Shares {
                shares: mounted_shares.clone(),
            };
            // forward to the GUI
            tx.send(StateUpdateToSession::MountUpdate {
                session_id: session_id.clone(),
                shares: SessionShares::Shares {
                    shares: mounted_shares,
                },
            })?;
            Ok(())
        }
    }
}

async fn recv_handle_lagging<T: Clone>(
    rx: &mut broadcast::Receiver<T>,
) -> Result<T, EventAPIError> {
    loop {
        match rx.recv().await {
            Ok(t) => return Ok(t),
            Err(e @ broadcast::error::RecvError::Closed) => {
                return Err(EventAPIError::ChannelRecv(e));
            }
            // recover from lagging state updates
            // FIXME: proper recovery, don't care for the MVP
            // (worst case, unlikely to happen, is that the session helper doesn't get a current
            // desired state message, which is only the case if all following messages are
            // DesktopSessionState)
            Err(_e @ broadcast::error::RecvError::Lagged(_n)) => continue,
        }
    }
}

async fn handle_session_helper_websocket(
    state: Arc<Mutex<AgentState>>,
    mut socket: WebSocket,
    session_id: SessionId,
) -> Result<(), EventAPIError> {
    let _guard = SessionConnection::new(
        &session_id,
        state.lock().await.session_channel_sender.clone(),
    )
    .await;
    let (mut rx, tx, remote_info, smb_hostname, smb_port) = {
        let agent_state = state.lock().await;
        let receiver = agent_state.desired_state_channel.subscribe();

        (
            receiver,
            agent_state.desired_state_channel.clone(),
            agent_state.remote_info.clone(),
            agent_state.config.smb_hostname.clone(),
            agent_state.config.smb_port,
        )
    };

    let session_config = SessionHelperConfig {
        smb_hostname,
        smb_port,
    };

    // initial state
    send_obj(
        &mut socket,
        &InternalMsgToSession::DesiredState {
            config: session_config.clone(),
            share_state: remote_info.map(|remote| Shares {
                foreground_share_id: remote
                    .desired_shares
                    .foreground_share
                    .clone()
                    .map(|s| s.share_id),
                shares: remote.desired_shares.shares.clone(),
            }),
        },
    )
    .await?;

    loop {
        select! {
            state_update = recv_handle_lagging(&mut rx) => {
                let update = state_update?;
                send_update_to_session_helper(update, &session_config, &mut socket).await?;
            }
            session_update = recv_obj::<InternalMsgToAgent>(&mut socket) => {
                record_update_from_session_helper(session_update?, tx.clone(), &session_id, state.lock().await.deref_mut()).await?;
            }
        }
    }
}

async fn send_update_to_gui(
    update: StateUpdateToSession,
    own_session_id: &SessionId,
    config: &SessionHelperConfig,
    socket: &mut WebSocket,
    remote_info: Option<RemoteInfo>,
) -> Result<(), EventAPIError> {
    let msg = match update {
        StateUpdateToSession::DesiredState {
            foreground_share_id,
            shares,
        } => InternalMsgToSession::DesiredState {
            config: config.clone(),
            share_state: Some(Shares {
                foreground_share_id,
                shares,
            }),
        },
        StateUpdateToSession::MountUpdate { session_id, shares } => {
            // ignore any updates that don't concern our session:
            if &session_id != own_session_id {
                return Ok(());
            }
            InternalMsgToSession::DesktopSessionState {
                session_id,
                mounted_shares: shares,
                desired_shares: remote_info.map(|remote| Shares {
                    foreground_share_id: remote
                        .desired_shares
                        .foreground_share
                        .clone()
                        .map(|s| s.share_id),
                    shares: remote.desired_shares.shares.clone(),
                }),
            }
        }
    };
    send_obj(socket, &msg).await?;
    Ok(())
}

async fn handle_gui_websocket(
    state: Arc<Mutex<AgentState>>,
    mut socket: WebSocket,
    session_id: SessionId,
) -> Result<(), EventAPIError> {
    let (mut rx, remote_info, smb_hostname, smb_port, mounted_shares) = {
        let agent_state = state.lock().await;
        let receiver = agent_state.desired_state_channel.subscribe();
        let mounted_shares = agent_state.mounted_shares.get(&session_id).cloned();

        (
            receiver,
            agent_state.remote_info.clone(),
            agent_state.config.smb_hostname.clone(),
            agent_state.config.smb_port,
            mounted_shares,
        )
    };

    let session_config = SessionHelperConfig {
        smb_hostname,
        smb_port,
    };

    let mounted_shares = mounted_shares.unwrap_or(SessionShares::NoSessionHelper);

    // initial state
    send_obj(
        &mut socket,
        &InternalMsgToSession::DesktopSessionState {
            session_id: session_id.clone(),
            mounted_shares,
            desired_shares: remote_info.map(|remote| Shares {
                foreground_share_id: remote
                    .desired_shares
                    .foreground_share
                    .clone()
                    .map(|s| s.share_id),
                shares: remote.desired_shares.shares.clone(),
            }),
        },
    )
    .await?;

    loop {
        select! {
            state_update = recv_handle_lagging(&mut rx) => {
                let update = state_update?;
                let remote_info = state.lock().await.remote_info.clone();
                send_update_to_gui(update, &session_id, &session_config, &mut socket, remote_info).await?;
            }
            session_update = recv_obj::<InternalMsgToAgent>(&mut socket) => {
                // panic! or rather, don't. the GUI isn't supposed to send us anything,
                // but if it does, just log and continue.
                let session_update = session_update?;
                warn!("GUI sent us something unexpected over the websocket connection: {session_update:?}");
            }
        }
    }
}

#[derive(Debug)]
pub enum SessionBeginEnd {
    Begin { session_id: SessionId },
    End { session_id: SessionId },
}

/// Session connection guard; when dropped, we send a message via the MPSC
/// channel which triggers cleanup in `agent_state::session_bookkeeper`
#[derive(Debug)]
struct SessionConnection {
    session_id: SessionId,
    channel: mpsc::UnboundedSender<SessionBeginEnd>,
}

impl SessionConnection {
    #[must_use]
    pub async fn new(
        session_id: &SessionId,
        channel: mpsc::UnboundedSender<SessionBeginEnd>,
    ) -> Self {
        #[allow(clippy::expect_used)]
        channel
            .send(SessionBeginEnd::Begin {
                session_id: session_id.clone(),
            })
            .expect("the session bookkeeping channel should be alive");
        SessionConnection {
            session_id: session_id.clone(),
            channel,
        }
    }
}

impl Drop for SessionConnection {
    fn drop(&mut self) {
        #[allow(clippy::expect_used)]
        self.channel
            .send(SessionBeginEnd::End {
                session_id: self.session_id.clone(),
            })
            .expect("session cleanup channel should be alive");
    }
}

async fn handle_connection(
    state: Arc<Mutex<AgentState>>,
    mut socket: WebSocket,
) -> Result<(), EventAPIError> {
    let msg_obj: InternalMsgToAgent = recv_obj(&mut socket).await?;

    match msg_obj {
        InternalMsgToAgent::HelloGui { session_id } => {
            handle_gui_websocket(state, socket, session_id).await?;
        }
        InternalMsgToAgent::HelloSessionHelper { session_id } => {
            handle_session_helper_websocket(state, socket, session_id).await?;
        }
        InternalMsgToAgent::SessionUpdate { shares: _ } => {
            return Err(EventAPIError::InvalidMsg);
        }
    }

    Ok(())
}

pub async fn events_api(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
) -> Response {
    // Disallow browsers from connecting. Our own clients don't send an Origin header.
    // this assumes that if the browser is too old to send an Origin header (IE10 or older),
    // it will not be connected to the internet, or at least not to untrusted websites.
    if headers.get("origin").is_some() {
        return StatusCode::FORBIDDEN.into_response();
    }

    ws.on_upgrade(async move |socket| {
        if let Err(e) = handle_connection(Arc::clone(&state.state), socket).await {
            error!("dropping websocket connection: {e}");
        }
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use axum::{Router, routing::get};
    use futures_util::{SinkExt, StreamExt};
    use minijinja::Environment;
    use modama_common::{
        messages::Share,
        tokens::{BoxFutureToken, TokenProvider, TokenSignResponse},
    };
    use rustls::{ClientConfig, RootCertStore};
    use serde::{Serialize, de::DeserializeOwned};
    use tokio::{
        net::{TcpListener, TcpStream},
        task::JoinHandle,
        time::timeout,
    };
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
    use tungstenite::{
        Error as WebSocketError, Message as WebSocketMessage, client::IntoClientRequest,
    };
    use url::Url;

    use super::*;

    type TestWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

    #[derive(Debug)]
    struct DummyTokenProvider;

    impl TokenProvider for DummyTokenProvider {
        fn fetch_token(self: Arc<Self>) -> BoxFutureToken {
            Box::pin(async move {
                Ok(TokenSignResponse {
                    token: "dummy-token".to_owned(),
                    resource: "resource".to_owned(),
                    agent_name: "agent-name".to_owned(),
                    agent_uri: "x-modama-agent://resource#agent-name".to_owned(),
                })
            })
        }
    }

    struct TestSetup {
        server_task: JoinHandle<()>,
        shared_state: Arc<Mutex<AgentState>>,
        websocket_url: String,
    }

    impl TestSetup {
        async fn start() -> Self {
            let root = RootCertStore::empty();
            let config = crate::config::AgentConfig {
                events_api_url: Url::parse("ws://localhost/").unwrap(),
                mgmt_api_url: Url::parse("http://localhost/").unwrap(),
                smb_hostname: "smb.example.test".to_owned(),
                smb_ip: tokio::sync::OnceCell::new(),
                smb_port: 1445,
                smb_domain: "WORKGROUP".to_owned(),
                identity: crate::config::MTLSIdentity::AgentId {
                    agent_name: "agent-name".to_owned(),
                    resource_name: "resource".to_owned(),
                },
                linux_config: None,
                listen_local_api: "localhost:0".to_owned(),
                token_service_url: Url::parse("http://localhost/").unwrap(),
                ca_root_store: root.clone(),
                ca_root_cert: String::new(),
                client_config: Arc::new(
                    ClientConfig::builder()
                        .with_root_certificates(root)
                        .with_no_client_auth(),
                ),
                client_cert: None,
            };
            let shared_state = AgentState::new(&config).await;
            let token_provider: Arc<dyn TokenProvider + Send + Sync> = Arc::new(DummyTokenProvider);
            let api_state = ApiState {
                state: Arc::clone(&shared_state),
                token_provider,
                jinja_env: Environment::new(),
            };
            let app = Router::new()
                .route("/internal/events/", get(events_api))
                .with_state(api_state);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server_task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });

            Self {
                server_task,
                shared_state,
                websocket_url: format!("ws://{address}/internal/events/"),
            }
        }

        async fn connect(&self) -> TestWebSocket {
            connect_async(&self.websocket_url).await.unwrap().0
        }

        async fn connect_gui(&self, id: &str) -> TestWebSocket {
            let mut socket = self.connect().await;
            send_message(
                &mut socket,
                &InternalMsgToAgent::HelloGui {
                    session_id: session_id(id),
                },
            )
            .await;
            socket
        }

        async fn connect_session_helper(&self, id: &str) -> TestWebSocket {
            let mut socket = self.connect().await;
            send_message(
                &mut socket,
                &InternalMsgToAgent::HelloSessionHelper {
                    session_id: session_id(id),
                },
            )
            .await;
            socket
        }

        async fn seed_remote_info(&self, foreground_share: Share, shares: Vec<Share>) {
            let mut remote_info = RemoteInfo::new();
            remote_info
                .desired_shares
                .replace_state(shares, Some(foreground_share));
            self.shared_state.lock().await.remote_info = Some(remote_info);
        }

        async fn seed_mounted_shares(&self, id: &str, shares: Vec<MountedShare>) {
            self.shared_state
                .lock()
                .await
                .mounted_shares
                .insert(session_id(id), SessionShares::Shares { shares });
        }

        async fn wait_for_state(&self, predicate: impl Fn(&AgentState) -> bool) {
            timeout(Duration::from_secs(1), async {
                loop {
                    let state = self.shared_state.lock().await;
                    if predicate(&state) {
                        return;
                    }
                    drop(state);
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("agent state did not reach the expected value");
        }
    }

    impl Drop for TestSetup {
        fn drop(&mut self) {
            self.server_task.abort();
        }
    }

    fn session_id(id: &str) -> SessionId {
        SessionId { id: id.to_owned() }
    }

    fn share(id: &str) -> Share {
        Share {
            resource: Some("resource".to_owned()),
            share_id: id.to_owned(),
            share_name: format!("share-{id}"),
            dest_path: format!("/project/{id}"),
            created_timestamp: 0,
            username: "share-user".to_owned(),
            password: "share-password".to_owned(),
            uid: 1000,
            gid: 1000,
        }
    }

    async fn send_message<T: Serialize>(socket: &mut TestWebSocket, message: &T) {
        socket
            .send(WebSocketMessage::Text(
                serde_json::to_string(message).unwrap().into(),
            ))
            .await
            .unwrap();
    }

    async fn receive_message<T: DeserializeOwned>(socket: &mut TestWebSocket) -> T {
        let message = timeout(Duration::from_secs(1), socket.next())
            .await
            .expect("timed out waiting for websocket message")
            .expect("websocket closed unexpectedly")
            .expect("websocket read failed");
        let text = message
            .into_text()
            .expect("expected websocket text message");
        serde_json::from_str(&text).unwrap()
    }

    async fn wait_for_termination(socket: &mut TestWebSocket) {
        timeout(Duration::from_secs(1), async {
            loop {
                match socket.next().await {
                    None | Some(Err(_)) | Some(Ok(WebSocketMessage::Close(_))) => return,
                    Some(Ok(WebSocketMessage::Ping(payload))) => {
                        socket.send(WebSocketMessage::Pong(payload)).await.unwrap();
                    }
                    Some(Ok(WebSocketMessage::Pong(_))) => {}
                    Some(Ok(message)) => {
                        panic!("received application response before termination: {message:?}")
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for websocket termination");
    }

    async fn wait_for_pong(socket: &mut TestWebSocket, expected_payload: &[u8]) {
        timeout(Duration::from_secs(1), async {
            loop {
                let message = socket
                    .next()
                    .await
                    .expect("websocket closed before pong arrived")
                    .expect("websocket read failed while waiting for pong");
                match message {
                    WebSocketMessage::Pong(payload) if payload.as_ref() == expected_payload => {
                        return;
                    }
                    WebSocketMessage::Pong(_) => {}
                    message => {
                        panic!("unexpected websocket message while waiting for pong: {message:?}")
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for websocket pong");
    }

    #[tokio::test]
    async fn browser_connections_are_denied() {
        let test_setup = TestSetup::start().await;
        let mut request = test_setup
            .websocket_url
            .clone()
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", "https://attacker.example".parse().unwrap());

        let error = connect_async(request).await.unwrap_err();

        match error {
            WebSocketError::Http(response) => {
                assert_eq!(response.status(), StatusCode::FORBIDDEN);
            }
            error => panic!("expected an HTTP rejection, got {error:?}"),
        }
    }

    #[tokio::test]
    async fn initial_empty_state_gui() {
        // connect as a GUI client and receive the initial (empty) state update (desired state and current
        // mount state)
        let test_setup = TestSetup::start().await;
        let mut gui = test_setup.connect_gui("desktop-1").await;

        let message: InternalMsgToSession = receive_message(&mut gui).await;

        match message {
            InternalMsgToSession::DesktopSessionState {
                session_id: actual_session_id,
                mounted_shares: SessionShares::NoSessionHelper,
                desired_shares: None,
            } => assert_eq!(actual_session_id, session_id("desktop-1")),
            message => panic!("unexpected initial GUI message: {message:?}"),
        }
    }

    #[tokio::test]
    async fn initial_empty_state_session_helper() {
        let test_setup = TestSetup::start().await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;

        let message: InternalMsgToSession = receive_message(&mut session_helper).await;

        match message {
            InternalMsgToSession::DesiredState {
                config,
                share_state: None,
            } => {
                assert_eq!(config.smb_hostname, "smb.example.test");
                assert_eq!(config.smb_port, 1445);
            }
            message => panic!("unexpected initial session-helper message: {message:?}"),
        }
    }

    #[tokio::test]
    async fn initial_non_empty_state_reaches_gui_and_session_helper() {
        let test_setup = TestSetup::start().await;
        let foreground_share = share("foreground");
        let background_share = share("background");
        let desired_shares = vec![foreground_share.clone(), background_share.clone()];
        let mounted_share = MountedShare {
            share: background_share.clone(),
            mount_path: PathBuf::from("/mnt/background"),
        };
        test_setup
            .seed_remote_info(foreground_share.clone(), desired_shares.clone())
            .await;
        test_setup
            .seed_mounted_shares("desktop-1", vec![mounted_share.clone()])
            .await;

        let mut gui = test_setup.connect_gui("desktop-1").await;
        let gui_message: InternalMsgToSession = receive_message(&mut gui).await;
        match gui_message {
            InternalMsgToSession::DesktopSessionState {
                session_id: actual_session_id,
                mounted_shares: SessionShares::Shares { shares },
                desired_shares:
                    Some(Shares {
                        foreground_share_id,
                        shares: actual_desired_shares,
                    }),
            } => {
                assert_eq!(actual_session_id, session_id("desktop-1"));
                assert_eq!(shares, vec![mounted_share]);
                assert_eq!(foreground_share_id.as_deref(), Some("foreground"));
                assert_eq!(actual_desired_shares, desired_shares);
            }
            message => panic!("unexpected initial GUI message: {message:?}"),
        }

        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let helper_message: InternalMsgToSession = receive_message(&mut session_helper).await;
        match helper_message {
            InternalMsgToSession::DesiredState {
                config,
                share_state:
                    Some(Shares {
                        foreground_share_id,
                        shares: actual_desired_shares,
                    }),
            } => {
                assert_eq!(config.smb_hostname, "smb.example.test");
                assert_eq!(config.smb_port, 1445);
                assert_eq!(foreground_share_id.as_deref(), Some("foreground"));
                assert_eq!(actual_desired_shares, desired_shares);
            }
            message => panic!("unexpected initial session-helper message: {message:?}"),
        }
    }

    #[tokio::test]
    async fn later_desired_state_reaches_session_helper() {
        let test_setup = TestSetup::start().await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut session_helper).await;
        let foreground_share = share("foreground");
        let background_share = share("background");

        test_setup
            .shared_state
            .lock()
            .await
            .desired_state_channel
            .send(StateUpdateToSession::DesiredState {
                foreground_share_id: Some(foreground_share.share_id.clone()),
                shares: vec![foreground_share.clone(), background_share.clone()],
            })
            .unwrap();

        let message: InternalMsgToSession = receive_message(&mut session_helper).await;
        match message {
            InternalMsgToSession::DesiredState {
                config,
                share_state:
                    Some(Shares {
                        foreground_share_id,
                        shares,
                    }),
            } => {
                assert_eq!(config.smb_hostname, "smb.example.test");
                assert_eq!(config.smb_port, 1445);
                assert_eq!(foreground_share_id.as_deref(), Some("foreground"));
                assert_eq!(shares, vec![foreground_share, background_share]);
            }
            message => panic!("unexpected session-helper update: {message:?}"),
        }
    }

    #[tokio::test]
    async fn update_from_session_helper_propagates() {
        let test_setup = TestSetup::start().await;
        let mut gui = test_setup.connect_gui("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut gui).await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut session_helper).await;
        let updated_share = share("share-1");

        send_message(
            &mut session_helper,
            &InternalMsgToAgent::SessionUpdate {
                shares: vec![SessionShare {
                    share: updated_share.clone(),
                    path: PathBuf::from("/mnt/share-1"),
                }],
            },
        )
        .await;

        let message: InternalMsgToSession = receive_message(&mut gui).await;
        match message {
            InternalMsgToSession::DesktopSessionState {
                session_id: actual_session_id,
                mounted_shares: SessionShares::Shares { shares },
                desired_shares: None,
            } => {
                assert_eq!(actual_session_id, session_id("desktop-1"));
                assert_eq!(shares.len(), 1);
                assert_eq!(shares[0].share, updated_share);
                assert_eq!(shares[0].mount_path, PathBuf::from("/mnt/share-1"));
            }
            message => panic!("unexpected GUI update: {message:?}"),
        }
        assert!(matches!(
            test_setup
                .shared_state
                .lock()
                .await
                .mounted_shares
                .get(&session_id("desktop-1")),
            Some(SessionShares::Shares { shares }) if shares.len() == 1
        ));
    }

    #[tokio::test]
    async fn mount_update_to_gui_includes_current_desired_state() {
        let test_setup = TestSetup::start().await;
        let foreground_share = share("foreground");
        let background_share = share("background");
        let desired_shares = vec![foreground_share.clone(), background_share];
        test_setup
            .seed_remote_info(foreground_share, desired_shares.clone())
            .await;
        let mut gui = test_setup.connect_gui("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut gui).await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut session_helper).await;
        let mounted_share = share("mounted");

        send_message(
            &mut session_helper,
            &InternalMsgToAgent::SessionUpdate {
                shares: vec![SessionShare {
                    share: mounted_share.clone(),
                    path: PathBuf::from("/mnt/mounted"),
                }],
            },
        )
        .await;

        let message: InternalMsgToSession = receive_message(&mut gui).await;
        match message {
            InternalMsgToSession::DesktopSessionState {
                session_id: actual_session_id,
                mounted_shares: SessionShares::Shares { shares },
                desired_shares:
                    Some(Shares {
                        foreground_share_id,
                        shares: actual_desired_shares,
                    }),
            } => {
                assert_eq!(actual_session_id, session_id("desktop-1"));
                assert_eq!(
                    shares,
                    vec![MountedShare {
                        share: mounted_share,
                        mount_path: PathBuf::from("/mnt/mounted"),
                    }]
                );
                assert_eq!(foreground_share_id.as_deref(), Some("foreground"));
                assert_eq!(actual_desired_shares, desired_shares);
            }
            message => panic!("unexpected GUI update: {message:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_initial_messages_terminate_connection_without_response() {
        let test_setup = TestSetup::start().await;
        let invalid_messages = [
            WebSocketMessage::Binary(vec![0, 1, 2].into()),
            WebSocketMessage::Text("{malformed-json".into()),
            WebSocketMessage::Text(
                serde_json::to_string(&InternalMsgToAgent::SessionUpdate { shares: vec![] })
                    .unwrap()
                    .into(),
            ),
        ];

        for message in invalid_messages {
            let mut socket = test_setup.connect().await;
            socket.send(message).await.unwrap();
            wait_for_termination(&mut socket).await;
        }
    }

    #[tokio::test]
    async fn repeated_session_helper_greeting_terminates_and_cleans_up() {
        let test_setup = TestSetup::start().await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut session_helper).await;
        send_message(
            &mut session_helper,
            &InternalMsgToAgent::SessionUpdate {
                shares: vec![SessionShare {
                    share: share("mounted"),
                    path: PathBuf::from("/mnt/mounted"),
                }],
            },
        )
        .await;
        test_setup
            .wait_for_state(|state| {
                state.mounted_shares.contains_key(&session_id("desktop-1"))
                    && state.session_connections.get(&session_id("desktop-1")) == Some(&1)
            })
            .await;

        send_message(
            &mut session_helper,
            &InternalMsgToAgent::HelloSessionHelper {
                session_id: session_id("desktop-1"),
            },
        )
        .await;
        wait_for_termination(&mut session_helper).await;

        test_setup
            .wait_for_state(|state| {
                !state.mounted_shares.contains_key(&session_id("desktop-1"))
                    && !state
                        .session_connections
                        .contains_key(&session_id("desktop-1"))
            })
            .await;
    }

    #[tokio::test]
    async fn session_state_cleaned_up_on_disconnect() {
        let test_setup = TestSetup::start().await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut session_helper).await;
        send_message(
            &mut session_helper,
            &InternalMsgToAgent::SessionUpdate { shares: vec![] },
        )
        .await;
        test_setup
            .wait_for_state(|state| {
                state.mounted_shares.contains_key(&session_id("desktop-1"))
                    && state.session_connections.get(&session_id("desktop-1")) == Some(&1)
            })
            .await;

        session_helper.close(None).await.unwrap();

        test_setup
            .wait_for_state(|state| {
                !state.mounted_shares.contains_key(&session_id("desktop-1"))
                    && !state
                        .session_connections
                        .contains_key(&session_id("desktop-1"))
            })
            .await;
    }

    #[tokio::test]
    async fn invalid_message_from_gui_are_ignored_and_bookkeeping_stays_alive() {
        let test_setup = TestSetup::start().await;
        let mut gui = test_setup.connect_gui("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut gui).await;

        send_message(
            &mut gui,
            &InternalMsgToAgent::SessionUpdate {
                shares: vec![SessionShare {
                    share: share("ignored-share"),
                    path: PathBuf::from("/mnt/ignored"),
                }],
            },
        )
        .await;
        let ping_payload = b"invalid-message-processed";
        gui.send(WebSocketMessage::Ping(
            ping_payload.as_slice().to_vec().into(),
        ))
        .await
        .unwrap();
        wait_for_pong(&mut gui, ping_payload).await;
        test_setup
            .shared_state
            .lock()
            .await
            .desired_state_channel
            .send(StateUpdateToSession::DesiredState {
                foreground_share_id: Some("desired-share".to_owned()),
                shares: vec![share("desired-share")],
            })
            .unwrap();

        let message: InternalMsgToSession = receive_message(&mut gui).await;
        match message {
            InternalMsgToSession::DesiredState {
                config,
                share_state:
                    Some(Shares {
                        foreground_share_id,
                        shares,
                    }),
            } => {
                assert_eq!(config.smb_hostname, "smb.example.test");
                assert_eq!(config.smb_port, 1445);
                assert_eq!(foreground_share_id.as_deref(), Some("desired-share"));
                assert_eq!(shares, vec![share("desired-share")]);
            }
            message => panic!("unexpected GUI update: {message:?}"),
        }
        assert!(
            test_setup
                .shared_state
                .lock()
                .await
                .mounted_shares
                .is_empty()
        );
    }

    #[tokio::test]
    async fn session_helper_disconnect_updates_state_to_discernable_value() {
        let test_setup = TestSetup::start().await;
        let mut gui = test_setup.connect_gui("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut gui).await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut session_helper).await;

        send_message(
            &mut session_helper,
            &InternalMsgToAgent::SessionUpdate { shares: vec![] },
        )
        .await;
        let mounted_message: InternalMsgToSession = receive_message(&mut gui).await;
        assert!(matches!(
            mounted_message,
            InternalMsgToSession::DesktopSessionState {
                mounted_shares: SessionShares::Shares { ref shares },
                ..
            } if shares.is_empty()
        ));

        session_helper.close(None).await.unwrap();

        let disconnected_message: InternalMsgToSession = receive_message(&mut gui).await;
        match disconnected_message {
            InternalMsgToSession::DesktopSessionState {
                session_id: actual_session_id,
                mounted_shares: SessionShares::NoSessionHelper,
                desired_shares: None,
            } => assert_eq!(actual_session_id, session_id("desktop-1")),
            message => panic!("unexpected disconnect update: {message:?}"),
        }
    }

    #[tokio::test]
    async fn updates_to_gui_connections_are_session_filtered() {
        let test_setup = TestSetup::start().await;
        let mut matching_gui = test_setup.connect_gui("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut matching_gui).await;
        let mut other_gui = test_setup.connect_gui("desktop-2").await;
        let _: InternalMsgToSession = receive_message(&mut other_gui).await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let _: InternalMsgToSession = receive_message(&mut session_helper).await;
        let updated_share = share("share-1");

        send_message(
            &mut session_helper,
            &InternalMsgToAgent::SessionUpdate {
                shares: vec![SessionShare {
                    share: updated_share.clone(),
                    path: PathBuf::from("/mnt/share-1"),
                }],
            },
        )
        .await;

        let message: InternalMsgToSession = receive_message(&mut matching_gui).await;
        match message {
            InternalMsgToSession::DesktopSessionState {
                session_id: actual_session_id,
                mounted_shares: SessionShares::Shares { shares },
                desired_shares: None,
            } => {
                assert_eq!(actual_session_id, session_id("desktop-1"));
                assert_eq!(
                    shares,
                    vec![MountedShare {
                        share: updated_share,
                        mount_path: PathBuf::from("/mnt/share-1"),
                    }]
                );
            }
            message => panic!("unexpected GUI update: {message:?}"),
        }

        let desired_share = share("desired-share");
        test_setup
            .shared_state
            .lock()
            .await
            .desired_state_channel
            .send(StateUpdateToSession::DesiredState {
                foreground_share_id: Some(desired_share.share_id.clone()),
                shares: vec![desired_share.clone()],
            })
            .unwrap();

        let message: InternalMsgToSession = receive_message(&mut other_gui).await;
        match message {
            InternalMsgToSession::DesiredState {
                share_state:
                    Some(Shares {
                        foreground_share_id,
                        shares,
                    }),
                ..
            } => {
                assert_eq!(foreground_share_id.as_deref(), Some("desired-share"));
                assert_eq!(shares, vec![desired_share]);
            }
            message => panic!("unexpected update for GUI in another session: {message:?}"),
        }
    }
}
