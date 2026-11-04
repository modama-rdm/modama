use std::sync::Arc;

use axum::{
    Router,
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::header::HeaderMap,
    response::{IntoResponse as _, Response},
    routing::get,
};
use hyper::StatusCode;
use log::{debug, error, warn};
use modama_common::internal_api_types::{
    SessionHelperConfig, SessionHelperState, ToAgent, ToAgentFromGui, ToSessionHelper,
};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{select, sync::Mutex};

use crate::{
    agent_state::AgentState,
    caller_scope::{ConnInfo, HelperScope},
    local_http_service::{ApiState, LocalAPIError},
};

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

async fn handle_session_helper_websocket(
    state: Arc<Mutex<AgentState>>,
    mut socket: WebSocket,
    scope: HelperScope,
) -> Result<(), EventAPIError> {
    let (fan_out, config) = {
        let state = state.lock().await;
        let config = SessionHelperConfig {
            smb_hostname: state.config.smb_hostname.clone(),
            smb_port: state.config.smb_port,
            smb_domain: state.config.smb_domain.clone(),
        };
        (Arc::clone(&state.fan_out), config)
    };
    // present until this connection ends; the first update is the current desired state
    let (presence, mut updates) = fan_out.connect_helper(scope);

    loop {
        select! {
            update = updates.next() => {
                send_obj(
                    &mut socket,
                    &ToSessionHelper {
                        config: config.clone(),
                        share_state: update.share_state,
                        force_release_obsolete: update.force_release_obsolete,
                    },
                )
                .await?;
            }
            session_update = recv_obj::<ToAgent>(&mut socket) => match session_update? {
                ToAgent::SessionUpdate {
                    shares,
                    errors,
                    reconciled_share_ids,
                } => presence.report(SessionHelperState {
                    mounted_shares: shares,
                    errors,
                    reconciled_share_ids,
                }),
            }
        }
    }
}

async fn handle_gui_websocket(
    state: Arc<Mutex<AgentState>>,
    mut socket: WebSocket,
    scope: HelperScope,
) -> Result<(), EventAPIError> {
    let fan_out = Arc::clone(&state.lock().await.fan_out);
    // the first view is what the GUI shows now
    let mut views = fan_out.subscribe_gui(scope);

    loop {
        select! {
            view = views.next() => {
                send_obj(&mut socket, &view).await?;
            }
            message = recv_obj::<serde_json::Value>(&mut socket) => {
                // malformed JSON ends the connection; a well-formed message we don't know is
                // only logged
                let message = message?;
                match serde_json::from_value::<ToAgentFromGui>(message.clone()) {
                    Ok(ToAgentFromGui::ForceRelease) => {
                        debug!("GUI of {scope:?} asks to force the release of obsolete mounts");
                        fan_out.request_force_release(scope);
                    }
                    Err(_) => warn!("GUI sent us something unexpected over the websocket connection: {message}"),
                }
            }
        }
    }
}

pub fn add_internal_api(router: Router<ApiState>) -> Result<Router<ApiState>, LocalAPIError> {
    Ok(router
        .route("/internal/events/gui/", get(events_api_gui))
        .route(
            "/internal/events/session-helper/",
            get(events_api_session_helper),
        ))
}

pub async fn events_api_gui(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
    ConnectInfo(conn): ConnectInfo<ConnInfo>,
) -> Response {
    // Disallow browsers from connecting. Our own clients don't send an Origin header.
    // this assumes that if the browser is too old to send an Origin header (IE10 or older),
    // it will not be connected to the internet, or at least not to untrusted websites.
    if headers.get("origin").is_some() {
        return StatusCode::FORBIDDEN.into_response();
    }

    // reject before the upgrade if we can't tell whom this connection belongs to:
    let scope = match state.resolve_scope(conn).await {
        Ok(scope) => scope,
        Err(e) => return e.into_response(),
    };

    ws.on_upgrade(async move |socket| {
        if let Err(e) = handle_gui_websocket(Arc::clone(&state.state), socket, scope).await {
            error!("dropping websocket connection: {e}");
        }
    })
    .into_response()
}

pub async fn events_api_session_helper(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
    ConnectInfo(conn): ConnectInfo<ConnInfo>,
) -> Response {
    // Disallow browsers from connecting. Our own clients don't send an Origin header.
    // this assumes that if the browser is too old to send an Origin header (IE10 or older),
    // it will not be connected to the internet, or at least not to untrusted websites.
    if headers.get("origin").is_some() {
        return StatusCode::FORBIDDEN.into_response();
    }

    // reject before the upgrade if we can't tell whom this connection belongs to:
    let scope = match state.resolve_scope(conn).await {
        Ok(scope) => scope,
        Err(e) => return e.into_response(),
    };

    ws.on_upgrade(async move |socket| {
        if let Err(e) =
            handle_session_helper_websocket(Arc::clone(&state.state), socket, scope).await
        {
            error!("dropping websocket connection: {e}");
        }
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

    use axum::Router;
    use futures_util::{SinkExt, StreamExt};
    use minijinja::Environment;
    use modama_common::{
        internal_api_types::{MountedShare, ShareError, Shares, ToSessionGui},
        messages::{AgentMountStatus, Share},
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
    use crate::{caller_scope::QueueScopeResolver, local_fan_out::LocalFanOut};

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
        scopes: Arc<QueueScopeResolver>,
        address: SocketAddr,
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
            let shared_state = AgentState::new(&config);
            let token_provider: Arc<dyn TokenProvider + Send + Sync> = Arc::new(DummyTokenProvider);
            let scopes = Arc::new(QueueScopeResolver::default());
            let api_state = ApiState {
                state: Arc::clone(&shared_state),
                token_provider,
                jinja_env: Environment::new(),
                scope_resolver: Arc::clone(&scopes) as Arc<dyn crate::caller_scope::ResolveScope>,
                mgmt_api: Arc::new(crate::mgmt_api::MgmtApi::new()),
            };
            let app = add_internal_api(Router::new())
                .unwrap()
                .with_state(api_state);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server_task = tokio::spawn(async move {
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<ConnInfo>(),
                )
                .await
                .unwrap();
            });

            Self {
                server_task,
                shared_state,
                scopes,
                address,
            }
        }

        async fn connect(&self, url: &str) -> TestWebSocket {
            connect_async(url).await.unwrap().0
        }

        fn websocket_gui(&self) -> String {
            format!("ws://{}/internal/events/gui/", self.address)
        }

        fn websocket_helper(&self) -> String {
            format!("ws://{}/internal/events/session-helper/", self.address)
        }

        /// Connect as a GUI that the agent will find to be in scope `id`
        async fn connect_gui(&self, id: &str) -> TestWebSocket {
            self.scopes.push(scope(id));
            self.connect(&self.websocket_gui()).await
        }

        /// Connect as a session helper that the agent will find to be in scope `id`
        async fn connect_session_helper(&self, id: &str) -> TestWebSocket {
            self.scopes.push(scope(id));
            self.connect(&self.websocket_helper()).await
        }

        async fn fan_out(&self) -> Arc<LocalFanOut> {
            Arc::clone(&self.shared_state.lock().await.fan_out)
        }

        /// smbdrop sends `shares`, `foreground` among them, and no participants
        async fn smbdrop_sends(&self, foreground: Option<Share>, shares: Vec<Share>) {
            self.fan_out()
                .await
                .apply_share_list(shares, foreground, vec![]);
        }
    }

    impl Drop for TestSetup {
        fn drop(&mut self) {
            self.server_task.abort();
        }
    }

    /// Tests name scopes; on the agent's side, each name is a distinct uid
    fn scope(id: &str) -> HelperScope {
        match id {
            "desktop-1" => HelperScope::Uid(1001),
            "desktop-2" => HelperScope::Uid(1002),
            "s1" => HelperScope::Uid(1),
            _ => panic!("unknown test scope {id}"),
        }
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
            created_by: None,
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
        for url in [test_setup.websocket_gui(), test_setup.websocket_helper()] {
            test_setup.scopes.push(scope("desktop-1"));
            let mut request = url.into_client_request().unwrap();
            request
                .headers_mut()
                .insert("origin", "https://attacker.example".parse().unwrap());

            match connect_async(request).await.unwrap_err() {
                WebSocketError::Http(response) => {
                    assert_eq!(response.status(), StatusCode::FORBIDDEN);
                }
                error => panic!("expected an HTTP rejection, got {error:?}"),
            }
        }
    }

    #[tokio::test]
    async fn unknown_scope_is_rejected_before_upgrade() {
        // nothing queued in the scope resolver: the agent can't tell whom this is
        let test_setup = TestSetup::start().await;
        for url in [test_setup.websocket_gui(), test_setup.websocket_helper()] {
            match connect_async(url).await.unwrap_err() {
                WebSocketError::Http(response) => {
                    assert_eq!(response.status(), StatusCode::FORBIDDEN);
                }
                error => panic!("expected an HTTP rejection, got {error:?}"),
            }
        }
        // no session helper counts as connected
        test_setup.smbdrop_sends(None, vec![]).await;
        assert_eq!(
            test_setup.fan_out().await.mount_status(),
            Some(AgentMountStatus::NoHelper)
        );
    }

    #[tokio::test]
    async fn initial_empty_state_gui() {
        // connect as a GUI client and receive the initial (empty) state update (desired state and current
        // mount state)
        let test_setup = TestSetup::start().await;
        let mut gui = test_setup.connect_gui("desktop-1").await;
        let message: ToSessionGui = receive_message(&mut gui).await;
        assert_eq!(message.session_helper, None);
        assert!(message.desired_shares.is_none());
        assert!(message.connected_agents.is_none());
    }

    #[tokio::test]
    async fn initial_empty_state_session_helper() {
        let test_setup = TestSetup::start().await;
        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;

        let message: ToSessionHelper = receive_message(&mut session_helper).await;
        assert_eq!(message.config.smb_hostname, "smb.example.test");
        assert_eq!(message.config.smb_port, 1445);
        assert_eq!(message.config.smb_domain, "WORKGROUP");
        assert!(message.share_state.is_none());
        assert!(!message.force_release_obsolete);
    }

    #[tokio::test]
    async fn initial_non_empty_state_reaches_gui_and_session_helper() {
        let test_setup = TestSetup::start().await;
        let foreground_share = share("foreground");
        let background_share = share("background");
        let desired_shares = vec![foreground_share.clone(), background_share.clone()];
        let mounted_share = MountedShare {
            share_name: background_share.share_name,
            mount_path: Some(PathBuf::from("/mnt/background")),
        };
        test_setup
            .smbdrop_sends(Some(foreground_share.clone()), desired_shares.clone())
            .await;
        let (helper, _) = test_setup
            .fan_out()
            .await
            .connect_helper(scope("desktop-1"));
        helper.report(SessionHelperState {
            mounted_shares: vec![mounted_share.clone()],
            errors: vec![],
            reconciled_share_ids: None,
        });

        let mut gui = test_setup.connect_gui("desktop-1").await;
        let gui_message: ToSessionGui = receive_message(&mut gui).await;
        assert_eq!(
            gui_message.session_helper.map(|h| h.mounted_shares),
            Some(vec![mounted_share])
        );
        assert_eq!(
            gui_message.desired_shares,
            Some(Shares {
                foreground_share_id: Some("foreground".to_owned()),
                shares: desired_shares.clone(),
            })
        );

        let mut session_helper = test_setup.connect_session_helper("desktop-1").await;
        let helper_message: ToSessionHelper = receive_message(&mut session_helper).await;
        assert_eq!(helper_message.config.smb_hostname, "smb.example.test");
        assert_eq!(
            helper_message.share_state,
            Some(Shares {
                foreground_share_id: Some("foreground".to_owned()),
                shares: desired_shares,
            })
        );
    }

    /// One helper and one GUI of a scope, from smbdrop's update to the helper's report, the
    /// GUI's force release, and the helper going away
    #[tokio::test]
    async fn helper_and_gui_end_to_end() {
        let test_setup = TestSetup::start().await;
        let mut gui = test_setup.connect_gui("desktop-1").await;
        let _: ToSessionGui = receive_message(&mut gui).await;
        let mut helper = test_setup.connect_session_helper("desktop-1").await;
        let _: ToSessionHelper = receive_message(&mut helper).await;
        // the helper is connected, but until smbdrop sends a share list our status is unknown,
        // so the GUI's view doesn't change yet

        let desired = share("desired");
        test_setup
            .smbdrop_sends(Some(desired.clone()), vec![desired.clone()])
            .await;
        let shares = Shares {
            foreground_share_id: Some("desired".to_owned()),
            shares: vec![desired.clone()],
        };
        let message: ToSessionHelper = receive_message(&mut helper).await;
        assert_eq!(message.share_state, Some(shares.clone()));
        assert!(!message.force_release_obsolete);
        let message: ToSessionGui = receive_message(&mut gui).await;
        assert_eq!(message.desired_shares, Some(shares.clone()));
        assert_eq!(message.session_helper, None);

        let report = SessionHelperState {
            mounted_shares: vec![MountedShare {
                share_name: desired.share_name.clone(),
                mount_path: Some(PathBuf::from("/mnt/desired")),
            }],
            errors: vec![ShareError {
                share_id: "failed".to_owned(),
                message: "credential conflict".to_owned(),
            }],
            reconciled_share_ids: Some(vec!["desired".to_owned()]),
        };
        send_message(
            &mut helper,
            &ToAgent::SessionUpdate {
                shares: report.mounted_shares.clone(),
                errors: report.errors.clone(),
                reconciled_share_ids: report.reconciled_share_ids.clone(),
            },
        )
        .await;
        let message: ToSessionGui = receive_message(&mut gui).await;
        assert_eq!(message.session_helper, Some(report));
        assert_eq!(message.desired_shares, Some(shares.clone()));

        send_message(&mut gui, &ToAgentFromGui::ForceRelease).await;
        let message: ToSessionHelper = receive_message(&mut helper).await;
        assert!(message.force_release_obsolete);
        assert_eq!(message.share_state, Some(shares));

        helper.close(None).await.unwrap();
        let message: ToSessionGui = receive_message(&mut gui).await;
        assert_eq!(message.session_helper, None);
        assert_eq!(
            test_setup.fan_out().await.mount_status(),
            Some(AgentMountStatus::NoHelper)
        );
    }

    #[tokio::test]
    async fn invalid_messages_terminate_connection() {
        let test_setup = TestSetup::start().await;
        let invalid_messages = [
            WebSocketMessage::Binary(vec![0, 1, 2].into()),
            WebSocketMessage::Text("{malformed-json".into()),
        ];

        // both endpoints send the initial state right away, only then do we send garbage:
        for message in invalid_messages.clone() {
            let mut socket = test_setup.connect_gui("s1").await;
            let _: ToSessionGui = receive_message(&mut socket).await;
            socket.send(message).await.unwrap();
            wait_for_termination(&mut socket).await;
        }

        for message in invalid_messages {
            let mut socket = test_setup.connect_session_helper("s1").await;
            let _: ToSessionHelper = receive_message(&mut socket).await;
            socket.send(message).await.unwrap();
            wait_for_termination(&mut socket).await;
        }
    }

    #[tokio::test]
    async fn unknown_messages_from_a_gui_are_ignored() {
        let test_setup = TestSetup::start().await;
        let mut gui = test_setup.connect_gui("desktop-1").await;
        let _: ToSessionGui = receive_message(&mut gui).await;

        // a helper's report, from a GUI
        send_message(
            &mut gui,
            &ToAgent::SessionUpdate {
                shares: vec![MountedShare {
                    share_name: share("ignored-share").share_name,
                    mount_path: Some(PathBuf::from("/mnt/ignored")),
                }],
                errors: vec![],
                reconciled_share_ids: None,
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
            .smbdrop_sends(Some(share("desired-share")), vec![share("desired-share")])
            .await;

        // the connection is still alive, and what the GUI sent doesn't show up as helper state:
        let message: ToSessionGui = receive_message(&mut gui).await;
        assert_eq!(message.session_helper, None);
        assert_eq!(
            message.desired_shares,
            Some(Shares {
                foreground_share_id: Some("desired-share".to_owned()),
                shares: vec![share("desired-share")]
            })
        );
    }
}
