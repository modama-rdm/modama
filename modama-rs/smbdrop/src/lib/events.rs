use std::{io, net::SocketAddr, path::PathBuf, sync::Arc};

use crate::{
    event_hub::{ConnectionRegistration, EventHub, ResourceUpdates},
    helpers::peer_agent_id,
    token_server::LocalTokenVerifier,
};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use modama_common::{
    agent::{AgentId, ClientCertError},
    cert_helpers::CertHelperError,
    messages::{
        EVENT_CHANNEL_PROTOCOL, EventChannelInitialMessages, EventChannelMessage,
        EventChannelRequests, MAX_ANONYMOUS_NAME_CHARS, MIN_EVENT_CHANNEL_PROTOCOL,
        ParticipantIdentity,
    },
};
use rustls::ServerConfig;
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, rustls::server::VerifierBuilderError, server::TlsStream};
use tokio_tungstenite::{WebSocketStream, accept_async};
use tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

#[derive(thiserror::Error, Debug)]
pub enum ServerStartError {
    #[error("listen failed")]
    ListenError,

    #[error("certificate error: {0}")]
    CertError(Box<dyn std::error::Error + Sync + Send>),

    #[error("i/o error while reading {path}: {source}")]
    IoError { source: io::Error, path: PathBuf },
}

impl From<std::io::Error> for ServerStartError {
    fn from(value: std::io::Error) -> Self {
        ServerStartError::CertError(Box::new(value))
    }
}

impl From<CertHelperError> for ServerStartError {
    fn from(value: CertHelperError) -> Self {
        ServerStartError::CertError(Box::new(value))
    }
}

impl From<tokio_rustls::rustls::Error> for ServerStartError {
    fn from(value: tokio_rustls::rustls::Error) -> Self {
        ServerStartError::CertError(Box::new(value))
    }
}

impl From<VerifierBuilderError> for ServerStartError {
    fn from(value: VerifierBuilderError) -> Self {
        ServerStartError::CertError(Box::new(value))
    }
}

#[derive(thiserror::Error, Debug)]
enum HandlerError {
    #[error("websocket communication failed: {0}")]
    WsError(#[from] tungstenite::Error),

    #[error("serialization failure: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("Authentication failed: {0}")]
    Auth(String),
}

async fn send_update(
    ws_stream: &mut WebSocketStream<TlsStream<TcpStream>>,
    update: &EventChannelMessage,
) -> Result<(), HandlerError> {
    let json_msg = serde_json::to_string(update)?;
    ws_stream
        .send(tungstenite::Message::Text(json_msg.into()))
        .await?;
    Ok(())
}

async fn handle_ws_message(
    msg: &str,
    ws_stream: &mut WebSocketStream<TlsStream<TcpStream>>,
    registration: &ConnectionRegistration,
    updates: &mut ResourceUpdates,
) -> Result<(), HandlerError> {
    let connection = registration.id();
    // requests from newer agents that we don't know: keep the connection
    let msg: EventChannelRequests = match serde_json::from_str(msg) {
        Ok(msg) => msg,
        Err(e) => {
            warn!("ignoring request on connection {connection} that we don't understand: {e}");
            return Ok(());
        }
    };

    match msg {
        EventChannelRequests::ShareListUpdate => {
            send_update(ws_stream, &updates.current()).await?;
        }
        EventChannelRequests::ReportMountStatus { status } => {
            registration.report_status(status);
        }
    }

    Ok(())
}

/// Take care of a single websocket connection. This function has two purposes:
///
/// 1) Receive and reply to websockets sent to us from the agent on the other end
/// 2) Send the agent the updates for its resource as the event hub changes
async fn handle_connection(
    ws_stream: &mut WebSocketStream<TlsStream<TcpStream>>,
    registration: &ConnectionRegistration,
    mut updates: ResourceUpdates,
) -> Result<(), HandlerError> {
    let connection = registration.id();
    loop {
        select! {
            msg = ws_stream.next() => {
                let msg = if let Some(msg) = msg { msg? } else { break };
                if let Ok(msg) = msg.into_text() {
                    info!("connection {connection}: got ws message: {msg}");
                    handle_ws_message(&msg, ws_stream, registration, &mut updates).await?;
                } else {
                    warn!("received non-text ws message or conversion failed");
                }
            },
            update = updates.next() => {
                debug!("connection {connection}: sending update {update:?}");
                send_update(ws_stream, &update).await?;
            }
        }
    }

    Ok(())
}

#[derive(thiserror::Error, Debug)]
enum AcceptError {
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Websocket I/O failed: {0}")]
    MessagingError(#[from] tungstenite::Error),
}

/// What the agent told us about itself when logging in
struct ClientInfo<'a> {
    version: &'a str,

    /// The highest event channel protocol version the agent speaks
    protocol: u32,
}

async fn handle_authenticated_connection(
    peer: SocketAddr,
    client: ClientInfo<'_>,
    mut ws_stream: WebSocketStream<TlsStream<TcpStream>>,
    identity: ParticipantIdentity,
    hub: &EventHub,
) -> Result<(), HandlerError> {
    let updates = hub.subscribe(identity.resource());
    let description = match &identity {
        ParticipantIdentity::Certified { agent } => {
            format!("certified agent {}", agent.san_encoded)
        }
        ParticipantIdentity::Anonymous { name, lender } => {
            format!(
                "anonymous agent {name:?} with a token from {}",
                lender.san_encoded
            )
        }
    };

    // from here on, the connection is listed until `registration` is dropped
    let registration = hub.register(identity, peer);
    let connection = registration.id();
    info!(
        "connection {connection} from {peer}: {description}, version {} protocol {}",
        client.version, client.protocol
    );

    // before any update, so the agent knows which protocol version to use from the start:
    let welcome = EventChannelMessage::Welcome {
        protocol: client.protocol.min(EVENT_CHANNEL_PROTOCOL),
        connection,
    };
    ws_stream
        .send(tungstenite::Message::Text(
            serde_json::to_string(&welcome)?.into(),
        ))
        .await?;

    if let Err(e) = handle_connection(&mut ws_stream, &registration, updates).await {
        match e {
            HandlerError::WsError(tungstenite::Error::ConnectionClosed)
            | HandlerError::WsError(tungstenite::Error::Protocol(_))
            | HandlerError::WsError(tungstenite::Error::Utf8(_)) => (),
            err => error!("Error handling connection {connection}: {err}"),
        }
    }

    Ok(())
}

/// Why smbdrop turns a login away; logged, and sent to the agent as the close reason
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
enum LoginRejection {
    #[error(
        "protocol {protocol} is too old, smbdrop needs {MIN_EVENT_CHANNEL_PROTOCOL}; update the agent"
    )]
    ProtocolTooOld { protocol: u32 },

    #[error("token verification failed")]
    InvalidToken,

    #[error("the client certificate names no agent: {0}")]
    InvalidCertificate(String),

    #[error("token for {token}, client certificate for {certificate}")]
    TokenNotForCertificate { token: String, certificate: String },

    #[error("Login without a client certificate; agents without one use AnonymousLogin")]
    LoginWithoutCertificate,

    #[error("AnonymousLogin with a client certificate; agents with one use Login")]
    AnonymousWithCertificate,
}

/// Who a login is from, by the rules of ADR 0001: the client certificate identifies an agent,
/// the token authorizes it at the token's resource. `anonymous_name` is the name sent with an
/// `AnonymousLogin`, `None` for a `Login`.
fn identify(
    token_agent: AgentId,
    certificate: Result<Option<AgentId>, ClientCertError>,
    anonymous_name: Option<String>,
) -> Result<ParticipantIdentity, LoginRejection> {
    let certificate = certificate.map_err(|e| LoginRejection::InvalidCertificate(e.to_string()))?;
    match (certificate, anonymous_name) {
        (Some(certificate), None) if certificate == token_agent => {
            Ok(ParticipantIdentity::Certified { agent: certificate })
        }
        (Some(certificate), None) => Err(LoginRejection::TokenNotForCertificate {
            token: token_agent.san_encoded,
            certificate: certificate.san_encoded,
        }),
        (None, None) => Err(LoginRejection::LoginWithoutCertificate),
        (Some(_), Some(_)) => Err(LoginRejection::AnonymousWithCertificate),
        (None, Some(name)) => {
            let cut: String = name.chars().take(MAX_ANONYMOUS_NAME_CHARS).collect();
            if cut != name {
                warn!(
                    "anonymous agent's name is longer than {MAX_ANONYMOUS_NAME_CHARS} characters, listing it as {cut:?}"
                );
            }
            Ok(ParticipantIdentity::Anonymous {
                name: cut,
                lender: token_agent,
            })
        }
    }
}

/// Close the connection with `rejection` as the reason, which the agent logs
async fn reject_login(
    ws_stream: &mut WebSocketStream<TlsStream<TcpStream>>,
    rejection: &LoginRejection,
) -> Result<(), HandlerError> {
    // a close frame carries at most 123 bytes of reason
    let mut reason = rejection.to_string();
    while reason.len() > 123 {
        reason.pop();
    }
    ws_stream
        .close(Some(CloseFrame {
            code: CloseCode::Policy,
            reason: reason.into(),
        }))
        .await?;
    Ok(())
}

async fn authenticate_agent_connection(
    peer: SocketAddr,
    stream: TlsStream<TcpStream>,
    token_verifier: &LocalTokenVerifier,
    hub: &EventHub,
) -> Result<(), HandlerError> {
    let certificate = peer_agent_id(&stream);
    let mut ws_stream = accept_async(stream).await?;

    // the first message must be the authentication, otherwise we err out:
    let Some(msg) = ws_stream.next().await else {
        return Err(HandlerError::Auth(
            "first message could not be received".to_owned(),
        ));
    };
    let Ok(msg) = msg?.into_text() else {
        return Err(HandlerError::Auth("first message must be text".to_owned()));
    };
    let msg: EventChannelInitialMessages =
        serde_json::from_str(&msg).map_err(|e| HandlerError::Auth(e.to_string()))?;
    let (token, version, protocol, anonymous_name) = match msg {
        EventChannelInitialMessages::Login {
            token,
            version,
            protocol,
        } => (token, version, protocol, None),
        EventChannelInitialMessages::AnonymousLogin {
            token,
            version,
            protocol,
            name,
        } => (token, version, protocol, Some(name)),
    };
    debug!("connection attempt from {peer}; client version {version} protocol {protocol}");

    // from the login message, we gather the identity,
    // or reject the connection:
    let identity_or_rejection = if protocol < MIN_EVENT_CHANNEL_PROTOCOL {
        Err(LoginRejection::ProtocolTooOld { protocol })
    } else {
        token_verifier
            .verify(&token)
            .and_then(|claims| AgentId::from_uri(&claims.sub).ok())
            .ok_or(LoginRejection::InvalidToken)
            .and_then(|token_agent| identify(token_agent, certificate, anonymous_name))
    };
    match identity_or_rejection {
        Ok(identity) => {
            handle_authenticated_connection(
                peer,
                ClientInfo {
                    version: &version,
                    protocol,
                },
                ws_stream,
                identity,
                hub,
            )
            .await
        }
        Err(rejection) => {
            error!("rejecting connection from {peer}, agent version {version}: {rejection}");
            reject_login(&mut ws_stream, &rejection).await
        }
    }
}

async fn accept_agent_connection(
    peer: SocketAddr,
    stream: TcpStream,
    acceptor: TlsAcceptor,
    hub: EventHub,
    token_verifier: LocalTokenVerifier,
) -> Result<(), AcceptError> {
    let stream: TlsStream<TcpStream> = acceptor.accept(stream).await?;

    info!("connection from agent @ {peer}");

    if let Err(e) = authenticate_agent_connection(peer, stream, &token_verifier, &hub).await {
        error!("error in agent connection: {e:?}");
    } else {
        info!("finished handling connection from {peer}")
    }

    Ok(())
}

/// Start a websocket server that informs agents about the shares and the connections in `hub`.
/// Returns the server task and the port it listens on.
///
/// The given certificate must be enrolled via the CA as a server certificate.
pub async fn start_event_channel(
    listen_addr: &str,
    tls_config: ServerConfig,
    secret_key: &[u8],
    hub: EventHub,
) -> Result<(JoinHandle<()>, u16), ServerStartError> {
    let token_verifier = LocalTokenVerifier::new(secret_key);
    let listener = TcpListener::bind(&listen_addr).await.map_err(|e| {
        error!("listen error: {e}");
        ServerStartError::ListenError
    })?;
    let port = listener
        .local_addr()
        .map_err(|e| {
            error!("can't get port: {e}");
            ServerStartError::ListenError
        })?
        .port();
    let acceptor = TlsAcceptor::from(Arc::new(tls_config));

    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            if let Ok(peer) = stream.peer_addr() {
                tokio::spawn(accept_agent_connection(
                    peer,
                    stream,
                    acceptor.clone(),
                    hub.clone(),
                    token_verifier.clone(),
                ));
            } else {
                error!("no peer addr, dropping client");
            }
        }
    });

    Ok((task, port))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use std::{
        collections::HashMap,
        sync::Arc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use futures_util::{SinkExt, StreamExt};
    use modama_common::{
        agent::AgentId,
        ca::Ca,
        messages::{
            AgentMountStatus, ConnectionId, EVENT_CHANNEL_PROTOCOL, EventChannelInitialMessages,
            EventChannelMessage, EventChannelRequests, MAX_ANONYMOUS_NAME_CHARS, Participant,
            ParticipantIdentity, Share,
        },
        test_helpers::MockTokenProvider,
        tokens::{Claims, LocalTokenSigner, TokenProvider},
        version::modama_git_version,
    };
    use rustls::{
        ClientConfig, RootCertStore, ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
        server::WebPkiClientVerifier,
    };
    use tokio::{net, task::JoinHandle, time::timeout};
    use tokio_tungstenite::{
        Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    };
    use tungstenite::{Message, protocol::frame::coding::CloseCode};

    use super::identify;
    use crate::{event_hub::EventHub, events::start_event_channel};

    struct CertsForTesting {
        pub server_config: ServerConfig,
        pub server_secret: Vec<u8>,
        pub client_config_1: ClientConfig,
        pub client_config_2: ClientConfig,

        /// A client without a certificate, as anonymous agents connect
        pub client_config_anon: ClientConfig,
    }

    impl CertsForTesting {
        fn generate() -> Self {
            let ca = Ca::new::<String>(None, "hunter42").unwrap();
            let server_cert = ca.server_cert(&["localhost".to_string()]).unwrap();

            let mut roots = RootCertStore::empty();

            let ca_cert = ca.get_ca_cert_pem();
            roots
                .add(CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap())
                .unwrap();
            // like in production: clients with and without a certificate
            let client_cert_verifier = WebPkiClientVerifier::builder(roots.clone().into())
                .allow_unauthenticated()
                .build()
                .unwrap();

            // rustls expects the leaf (end-entity) certificate first, then any
            // intermediates; put the server certificate before the CA cert.
            let server_chain: Vec<CertificateDer> = vec![
                server_cert.cert.clone().into(),
                CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap(),
            ];

            // The serialized DER here is PKCS#8-formatted:
            let server_pk_der = rustls::pki_types::PrivateKeyDer::Pkcs8(
                server_cert.key_pair.serialize_der().into(),
            );

            let server_config: ServerConfig = ServerConfig::builder()
                .with_client_cert_verifier(client_cert_verifier)
                .with_single_cert(server_chain, server_pk_der.clone_key())
                .unwrap();

            let client_config_1 = {
                let client_cert = ca.client_cert("test_agent", "test_resource").unwrap();
                // leaf certificate first
                let client_chain: Vec<CertificateDer> = vec![
                    client_cert.cert.clone().into(),
                    CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap(),
                ];

                let client_pk_der =
                    PrivateKeyDer::Pkcs8(client_cert.key_pair.serialize_der().into());
                let client_config: ClientConfig = ClientConfig::builder()
                    .with_root_certificates(roots.clone())
                    .with_client_auth_cert(client_chain, client_pk_der)
                    .unwrap();
                client_config
            };

            let client_config_2 = {
                let client_cert = ca.client_cert("test_agent_2", "test_resource").unwrap();
                // leaf certificate first
                let client_chain: Vec<CertificateDer> = vec![
                    client_cert.cert.clone().into(),
                    CertificateDer::from_pem_slice(ca_cert.as_bytes()).unwrap(),
                ];

                let client_pk_der =
                    PrivateKeyDer::Pkcs8(client_cert.key_pair.serialize_der().into());
                let client_config: ClientConfig = ClientConfig::builder()
                    .with_root_certificates(roots.clone())
                    .with_client_auth_cert(client_chain, client_pk_der)
                    .unwrap();
                client_config
            };

            let client_config_anon = ClientConfig::builder()
                .with_root_certificates(roots.clone())
                .with_no_client_auth();

            CertsForTesting {
                server_config,
                client_config_1,
                client_config_2,
                client_config_anon,
                server_secret: server_pk_der.secret_der().to_vec(),
            }
        }
    }

    struct TestEventServer {
        pub hub: EventHub,
        pub ws_url: String,
        pub certs: CertsForTesting,
        pub handle: JoinHandle<()>,
    }

    async fn make_test_event_server() -> TestEventServer {
        let certs = CertsForTesting::generate();
        let hub = EventHub::new(HashMap::new());
        let (task, port) = start_event_channel(
            "localhost:0",
            certs.server_config.clone(),
            &certs.server_secret,
            hub.clone(),
        )
        .await
        .unwrap();
        TestEventServer {
            hub,
            ws_url: format!("wss://localhost:{port}"),
            certs,
            handle: task,
        }
    }

    /// Connect a certified agent of `test_resource` that is welcomed and gets its first update
    async fn connect_welcomed(
        server: &TestEventServer,
        client_config: &ClientConfig,
        agent: &str,
    ) -> WsStream {
        let mut conn = connect_agent(server, client_config, agent, EVENT_CHANNEL_PROTOCOL).await;
        assert!(matches!(
            next_msg(&mut conn).await,
            EventChannelMessage::Welcome { .. }
        ));
        next_msg(&mut conn).await;
        conn
    }

    #[tokio::test]
    async fn test_websocket_api_second_agent() {
        let server = make_test_event_server().await;
        let mut first =
            connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;

        let mut second = connect_agent(
            &server,
            &server.certs.client_config_2,
            "test_agent_2",
            EVENT_CHANNEL_PROTOCOL,
        )
        .await;
        assert_eq!(
            next_participants(&mut first).await,
            listed(&[("test_agent", None), ("test_agent_2", None)])
        );
        assert_no_msg(&mut first).await;

        second.close(None).await.unwrap();
        assert_eq!(
            next_participants(&mut first).await,
            listed(&[("test_agent", None)])
        );
        assert_no_msg(&mut first).await;
        server.handle.abort();
    }

    #[tokio::test]
    async fn test_websocket_api_request_share_update() {
        let server = make_test_event_server().await;
        let mut conn = connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;
        assert_no_msg(&mut conn).await;

        send_request(&mut conn, &EventChannelRequests::ShareListUpdate).await;
        assert_eq!(
            next_participants(&mut conn).await,
            listed(&[("test_agent", None)])
        );
        server.handle.abort();
    }

    #[tokio::test]
    async fn test_websocket_api_share_added() {
        let server = make_test_event_server().await;
        let mut conn = connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;

        let share_id = String::from("b4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let share = Share {
            resource: Some("test_resource".to_owned()),
            share_id: share_id.clone(),
            share_name: share_id.clone(),
            dest_path: "/dest/path".to_owned(),
            created_timestamp: 0,
            username: "testuser".to_owned(),
            password: "testpasswod".to_owned(),
            uid: 1001,
            gid: 1001,
            created_by: None,
        };
        server
            .hub
            .set_shares(HashMap::from([(share_id, share.clone())]));

        // the share manager informs us about the change, and the new share is in the list:
        let EventChannelMessage::UpdateShareList {
            shares,
            foreground_share,
            participants,
        } = next_msg(&mut conn).await
        else {
            panic!("expected an update");
        };
        assert_eq!(shares, vec![share.clone()]);
        assert_eq!(foreground_share, Some(share));
        assert_eq!(
            participants.iter().map(listing).collect::<Listed>(),
            listed(&[("test_agent", None)])
        );
        server.handle.abort();
    }

    type WsStream = WebSocketStream<MaybeTlsStream<net::TcpStream>>;

    fn agent_id(agent: &str) -> AgentId {
        AgentId {
            resource: "test_resource".to_owned(),
            agent: agent.to_owned(),
            san_encoded: format!("x-modama-agent://test_resource#{agent}"),
        }
    }

    /// Connect an agent of `test_resource` with a `Login` in the given protocol version and a
    /// token for `agent`
    async fn connect_agent(
        server: &TestEventServer,
        client_config: &ClientConfig,
        agent: &str,
        protocol: u32,
    ) -> WsStream {
        connect_with_login(server, client_config, agent, |token| {
            EventChannelInitialMessages::Login {
                token,
                version: modama_git_version().to_owned(),
                protocol,
            }
        })
        .await
    }

    /// Connect without a client certificate, with an `AnonymousLogin` named `name` and a
    /// token borrowed from `lender`
    async fn connect_anonymous(server: &TestEventServer, lender: &str, name: &str) -> WsStream {
        connect_with_login(server, &server.certs.client_config_anon, lender, |token| {
            EventChannelInitialMessages::AnonymousLogin {
                token,
                version: modama_git_version().to_owned(),
                protocol: EVENT_CHANNEL_PROTOCOL,
                name: name.to_owned(),
            }
        })
        .await
    }

    /// Connect with `client_config` and send the login that `login` makes from a token for
    /// `token_agent`
    async fn connect_with_login(
        server: &TestEventServer,
        client_config: &ClientConfig,
        token_agent: &str,
        login: impl FnOnce(String) -> EventChannelInitialMessages,
    ) -> WsStream {
        connect_with_token_for(server, client_config, &agent_id(token_agent), login).await
    }

    /// Connect with `client_config` and send the login that `login` makes from a token for
    /// `token_agent`, at any resource
    async fn connect_with_token_for(
        server: &TestEventServer,
        client_config: &ClientConfig,
        token_agent: &AgentId,
        login: impl FnOnce(String) -> EventChannelInitialMessages,
    ) -> WsStream {
        let (mut conn, _response) = connect_async_tls_with_config(
            server.ws_url.clone(),
            None,
            false,
            Some(Connector::Rustls(Arc::new(client_config.clone()))),
        )
        .await
        .unwrap();
        let token_provider: Arc<dyn TokenProvider + 'static> = Arc::new(MockTokenProvider {
            signer: LocalTokenSigner::new(&server.certs.server_secret),
            claims: Claims {
                sub: token_agent.san_encoded.clone(),
                exp: (SystemTime::now() + Duration::from_secs(1000))
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
        });
        let token = token_provider.fetch_token().await.unwrap().token;
        let login = serde_json::to_string(&login(token)).unwrap();
        conn.send(Message::Text(login.into())).await.unwrap();
        conn
    }

    /// The connection is closed with a reason containing `reason`
    async fn assert_rejected(conn: &mut WsStream, reason: &str) {
        let msg = timeout(Duration::from_millis(500), conn.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Close(Some(frame)) = msg else {
            panic!("expected a close frame, got {msg:?}");
        };
        assert_eq!(frame.code, CloseCode::Policy);
        assert!(
            frame.reason.contains(reason),
            "{:?} doesn't contain {reason:?}",
            frame.reason
        );
    }

    async fn next_msg(conn: &mut WsStream) -> EventChannelMessage {
        let msg = timeout(Duration::from_millis(500), conn.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str(&msg.into_text().unwrap()).unwrap()
    }

    async fn assert_no_msg(conn: &mut WsStream) {
        let res = timeout(Duration::from_millis(100), conn.next()).await;
        assert!(res.is_err(), "unexpected message: {res:?}");
    }

    async fn send_request(conn: &mut WsStream, request: &EventChannelRequests) {
        let text = serde_json::to_string(request).unwrap();
        conn.send(Message::Text(text.into())).await.unwrap();
    }

    type Listed = Vec<(String, Option<AgentMountStatus>)>;

    /// How `participant` is listed in tests: the agent name of a certified agent,
    /// "<name> via <lender>" for an anonymous one, and the status
    fn listing(participant: &Participant) -> (String, Option<AgentMountStatus>) {
        let label = match &participant.identity {
            ParticipantIdentity::Certified { agent } => agent.agent.clone(),
            ParticipantIdentity::Anonymous { name, lender } => {
                format!("{name} via {}", lender.agent)
            }
        };
        (label, participant.status)
    }

    /// The participants of the next message, an update without shares, as `listing` lists them
    async fn next_participants(conn: &mut WsStream) -> Listed {
        let EventChannelMessage::UpdateShareList {
            shares,
            participants,
            ..
        } = next_msg(conn).await
        else {
            panic!("expected an update");
        };
        assert!(shares.is_empty(), "unexpected shares: {shares:?}");
        participants.iter().map(listing).collect()
    }

    fn listed(participants: &[(&str, Option<AgentMountStatus>)]) -> Listed {
        participants
            .iter()
            .map(|(label, status)| ((*label).to_owned(), *status))
            .collect()
    }

    /// The connection ID in the next message, a `Welcome`
    async fn next_welcome(conn: &mut WsStream) -> ConnectionId {
        let EventChannelMessage::Welcome { connection, .. } = next_msg(conn).await else {
            panic!("expected a Welcome");
        };
        connection
    }

    #[tokio::test]
    async fn test_welcome_before_first_update() {
        let server = make_test_event_server().await;
        let mut conn = connect_agent(
            &server,
            &server.certs.client_config_1,
            "test_agent",
            EVENT_CHANNEL_PROTOCOL,
        )
        .await;
        let EventChannelMessage::Welcome {
            protocol,
            connection,
        } = next_msg(&mut conn).await
        else {
            panic!("expected a Welcome");
        };
        assert_eq!(protocol, EVENT_CHANNEL_PROTOCOL);
        let EventChannelMessage::UpdateShareList { participants, .. } = next_msg(&mut conn).await
        else {
            panic!("expected an update");
        };
        // the ID in the Welcome is the one this connection is listed under
        assert_eq!(participants.len(), 1);
        assert_eq!(participants[0].connection, connection);
        assert_no_msg(&mut conn).await;
        server.handle.abort();
    }

    /// An agent newer than us gets the protocol version we speak
    #[tokio::test]
    async fn test_welcome_negotiates_down() {
        let server = make_test_event_server().await;
        let mut conn = connect_agent(
            &server,
            &server.certs.client_config_1,
            "test_agent",
            EVENT_CHANNEL_PROTOCOL + 5,
        )
        .await;
        assert!(matches!(
            next_msg(&mut conn).await,
            EventChannelMessage::Welcome {
                protocol: EVENT_CHANNEL_PROTOCOL,
                ..
            }
        ));
        server.handle.abort();
    }

    #[tokio::test]
    async fn test_mount_status_is_broadcast_and_removed_on_disconnect() {
        let server = make_test_event_server().await;
        let mut agent_1 = connect_agent(
            &server,
            &server.certs.client_config_1,
            "test_agent",
            EVENT_CHANNEL_PROTOCOL,
        )
        .await;
        next_welcome(&mut agent_1).await;
        assert_eq!(
            next_participants(&mut agent_1).await,
            listed(&[("test_agent", None)])
        );

        let mut agent_2 = connect_agent(
            &server,
            &server.certs.client_config_2,
            "test_agent_2",
            EVENT_CHANNEL_PROTOCOL,
        )
        .await;
        next_welcome(&mut agent_2).await;
        let both = listed(&[("test_agent", None), ("test_agent_2", None)]);
        assert_eq!(next_participants(&mut agent_1).await, both);
        assert_eq!(next_participants(&mut agent_2).await, both);

        let report = EventChannelRequests::ReportMountStatus {
            status: AgentMountStatus::LeftoverWork,
        };
        send_request(&mut agent_1, &report).await;
        let expected = listed(&[
            ("test_agent", Some(AgentMountStatus::LeftoverWork)),
            ("test_agent_2", None),
        ]);
        assert_eq!(next_participants(&mut agent_1).await, expected);
        assert_eq!(next_participants(&mut agent_2).await, expected);

        // the same status again is not broadcast
        send_request(&mut agent_1, &report).await;
        assert_no_msg(&mut agent_2).await;

        // the status goes away with the agent
        agent_1.close(None).await.unwrap();
        assert_eq!(
            next_participants(&mut agent_2).await,
            listed(&[("test_agent_2", None)])
        );
        assert_no_msg(&mut agent_2).await;
        server.handle.abort();
    }

    /// Two connections with tokens for the same agent, as while smbdrop still holds an agent's
    /// old connection after it reconnected: each is listed with its own status, and closing one
    /// leaves the other
    #[tokio::test]
    async fn test_connections_with_one_identity_are_tracked_separately() {
        for close_first in [true, false] {
            let server = make_test_event_server().await;
            let mut first =
                connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;
            let mut second = connect_agent(
                &server,
                &server.certs.client_config_1,
                "test_agent",
                EVENT_CHANNEL_PROTOCOL,
            )
            .await;
            next_welcome(&mut second).await;
            let both = listed(&[("test_agent", None), ("test_agent", None)]);
            assert_eq!(next_participants(&mut first).await, both);
            assert_eq!(next_participants(&mut second).await, both);

            let report = |status| EventChannelRequests::ReportMountStatus { status };
            send_request(&mut first, &report(AgentMountStatus::Idle)).await;
            next_participants(&mut first).await;
            next_participants(&mut second).await;
            send_request(&mut second, &report(AgentMountStatus::NoHelper)).await;
            let reported = listed(&[
                ("test_agent", Some(AgentMountStatus::Idle)),
                ("test_agent", Some(AgentMountStatus::NoHelper)),
            ]);
            assert_eq!(next_participants(&mut first).await, reported);
            assert_eq!(next_participants(&mut second).await, reported);

            let (mut closing, mut staying, staying_status) = if close_first {
                (first, second, AgentMountStatus::NoHelper)
            } else {
                (second, first, AgentMountStatus::Idle)
            };
            closing.close(None).await.unwrap();
            assert_eq!(
                next_participants(&mut staying).await,
                listed(&[("test_agent", Some(staying_status))]),
                "close_first: {close_first}"
            );
            assert_no_msg(&mut staying).await;
            server.handle.abort();
        }
    }

    /// The certified agent and an anonymous agent with its token, as on the Windows test
    /// machine: both listed, each with its own status, whoever leaves first
    #[tokio::test]
    async fn test_anonymous_agent_and_its_lender_are_listed_separately() {
        let server = make_test_event_server().await;
        let mut lender =
            connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;
        let mut anonymous = connect_anonymous(&server, "test_agent", "neighbour-pc").await;
        next_welcome(&mut anonymous).await;
        next_participants(&mut lender).await;
        next_participants(&mut anonymous).await;

        let report = |status| EventChannelRequests::ReportMountStatus { status };
        send_request(&mut lender, &report(AgentMountStatus::Idle)).await;
        next_participants(&mut lender).await;
        next_participants(&mut anonymous).await;
        send_request(&mut anonymous, &report(AgentMountStatus::NoHelper)).await;
        let expected = listed(&[
            ("test_agent", Some(AgentMountStatus::Idle)),
            (
                "neighbour-pc via test_agent",
                Some(AgentMountStatus::NoHelper),
            ),
        ]);
        assert_eq!(next_participants(&mut lender).await, expected);
        assert_eq!(next_participants(&mut anonymous).await, expected);

        lender.close(None).await.unwrap();
        assert_eq!(
            next_participants(&mut anonymous).await,
            listed(&[(
                "neighbour-pc via test_agent",
                Some(AgentMountStatus::NoHelper)
            )])
        );
        server.handle.abort();
    }

    /// A connection that goes away without a closing handshake is removed all the same
    #[tokio::test]
    async fn test_connection_dropped_without_close_is_removed() {
        let server = make_test_event_server().await;
        let mut staying =
            connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;
        let mut dropped = connect_agent(
            &server,
            &server.certs.client_config_2,
            "test_agent_2",
            EVENT_CHANNEL_PROTOCOL,
        )
        .await;
        next_welcome(&mut dropped).await;
        let both = listed(&[("test_agent", None), ("test_agent_2", None)]);
        assert_eq!(next_participants(&mut staying).await, both);

        drop(dropped);
        assert_eq!(
            next_participants(&mut staying).await,
            listed(&[("test_agent", None)])
        );
        server.handle.abort();
    }

    /// A login that breaks the rules of ADR 0001 is closed with the reason, and nobody else
    /// hears of it
    #[tokio::test]
    async fn test_logins_are_checked_against_the_client_certificate() {
        let server = make_test_event_server().await;
        let mut observer =
            connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;
        let certs = &server.certs;

        let mut conn = connect_agent(
            &server,
            &certs.client_config_anon,
            "test_agent",
            EVENT_CHANNEL_PROTOCOL,
        )
        .await;
        assert_rejected(&mut conn, "Login without a client certificate").await;

        let mut conn = connect_agent(
            &server,
            &certs.client_config_2,
            "test_agent",
            EVENT_CHANNEL_PROTOCOL,
        )
        .await;
        assert_rejected(
            &mut conn,
            "token for x-modama-agent://test_resource#test_agent, client certificate for x-modama-agent://test_resource#test_agent_2",
        )
        .await;

        let mut conn =
            connect_with_login(&server, &certs.client_config_2, "test_agent_2", |token| {
                EventChannelInitialMessages::AnonymousLogin {
                    token,
                    version: modama_git_version().to_owned(),
                    protocol: EVENT_CHANNEL_PROTOCOL,
                    name: "pc".to_owned(),
                }
            })
            .await;
        assert_rejected(&mut conn, "AnonymousLogin with a client certificate").await;

        let mut conn = connect_agent(&server, &certs.client_config_2, "test_agent_2", 1).await;
        assert_rejected(&mut conn, "protocol 1 is too old").await;

        assert_no_msg(&mut observer).await;
        server.handle.abort();
    }

    /// Every connection hears only of the participants at its own resource, and only of changes
    /// there
    #[tokio::test]
    async fn test_participants_are_listed_per_resource() {
        let server = make_test_event_server().await;
        let mut here = connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;

        let elsewhere = AgentId {
            resource: "other_resource".to_owned(),
            agent: "remote".to_owned(),
            san_encoded: "x-modama-agent://other_resource#remote".to_owned(),
        };
        let mut far = connect_with_token_for(
            &server,
            &server.certs.client_config_anon,
            &elsewhere,
            |token| EventChannelInitialMessages::AnonymousLogin {
                token,
                version: modama_git_version().to_owned(),
                protocol: EVENT_CHANNEL_PROTOCOL,
                name: "far-pc".to_owned(),
            },
        )
        .await;
        next_welcome(&mut far).await;
        assert_eq!(
            next_participants(&mut far).await,
            listed(&[("far-pc via remote", None)])
        );
        // a connection elsewhere changes nothing here
        assert_no_msg(&mut here).await;
        server.handle.abort();
    }

    #[test]
    fn test_anonymous_names_are_cut() {
        let long = "ä".repeat(MAX_ANONYMOUS_NAME_CHARS + 6);
        let identity = identify(agent_id("test_agent"), Ok(None), Some(long)).unwrap();
        assert_eq!(
            identity,
            ParticipantIdentity::Anonymous {
                name: "ä".repeat(MAX_ANONYMOUS_NAME_CHARS),
                lender: agent_id("test_agent"),
            }
        );
    }

    /// Requests from newer agents that we don't understand don't end the connection
    #[tokio::test]
    async fn test_unknown_request_is_ignored() {
        let server = make_test_event_server().await;
        let mut conn = connect_welcomed(&server, &server.certs.client_config_1, "test_agent").await;

        conn.send(Message::Text(r#"{"type": "FromTheFuture"}"#.into()))
            .await
            .unwrap();
        send_request(&mut conn, &EventChannelRequests::ShareListUpdate).await;
        assert_eq!(
            next_participants(&mut conn).await,
            listed(&[("test_agent", None)])
        );
        server.handle.abort();
    }
}
