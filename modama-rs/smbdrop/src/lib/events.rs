use std::{collections::HashSet, io, net::SocketAddr, path::PathBuf, sync::Arc};

use crate::{share_manager::SmbDropState, token_server::LocalTokenVerifier};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use modama_common::{
    agent::{AgentId, ClientCertError, FromURIError},
    cert_helpers::CertHelperError,
    messages::{EventChannelInitialMessages, EventChannelMessage, EventChannelRequests, Share},
    tokens::Claims,
};
use rustls::ServerConfig;
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    sync::{
        Mutex,
        broadcast::{self, error::RecvError},
    },
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, rustls::server::VerifierBuilderError, server::TlsStream};
use tokio_tungstenite::{WebSocketStream, accept_async};

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

    #[error("Validate agent ID: {0}")]
    ClientCert(#[from] ClientCertError),

    #[error("Decode agent IRD: {0}")]
    FromURI(#[from] FromURIError),

    #[error("channel disconnected")]
    Disconnected,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EventSystemState {
    /// Shares and users updated from external (triggered via the HTTP API)
    pub inner_state: SmbDropState,

    /// Updated from the connection handler
    pub connected_agents: HashSet<AgentId>,
}

async fn handle_ws_message(
    msg: &str,
    ws_stream: &mut WebSocketStream<TlsStream<TcpStream>>,
    last_state: Arc<Mutex<EventSystemState>>,
    agent_id: &AgentId,
) -> Result<(), HandlerError> {
    let msg: EventChannelRequests = serde_json::from_str(msg)?;

    match msg {
        EventChannelRequests::ShareListUpdate => {
            let state = { last_state.lock().await.clone() };
            let msg = update_msg_from_state(&state, agent_id).await;
            let json_msg = serde_json::to_string(&msg)?;
            ws_stream
                .send(tungstenite::Message::Text(json_msg.into()))
                .await?;
        }
    }

    Ok(())
}

async fn update_msg_from_state(
    state: &EventSystemState,
    agent_id: &AgentId,
) -> EventChannelMessage {
    let mut shares = share_list_from_state(state, agent_id);
    shares.sort_by_key(|share| share.get_created_systime());

    // FIXME: right now, the foreground share is always the latest share
    // for the resource, by creation timestamp. This might need to be
    // re-evaluated.
    let mut connected_agents: Vec<AgentId> = state.connected_agents.iter().cloned().collect();
    connected_agents.sort_by(|id1, id2| id1.san_encoded.cmp(&id2.san_encoded));
    EventChannelMessage::UpdateShareList {
        shares: shares.clone(),
        foreground_share: shares.iter().last().cloned(),
        connected_agents,
    }
}

async fn handle_state_update(
    state: &EventSystemState,
    ws_stream: &mut WebSocketStream<TlsStream<TcpStream>>,
    agent_id: &AgentId,
) -> Result<(), HandlerError> {
    let msg = update_msg_from_state(state, agent_id).await;
    let json_msg = serde_json::to_string(&msg)?;
    ws_stream
        .send(tungstenite::Message::Text(json_msg.into()))
        .await?;

    Ok(())
}

fn share_list_from_state(state: &EventSystemState, agent_id: &AgentId) -> Vec<Share> {
    state
        .inner_state
        .shares
        .values()
        .filter(|s| {
            s.resource
                .as_ref()
                .is_some_and(|res| res == &agent_id.resource)
        })
        .cloned()
        .collect()
}

/// Take care of a single websocket connection. This function has two purposes:
///
/// 1) Receive and reply to websockets sent to us from the agent on the other end
/// 2) Send out state updates via the websocket as they come in from the
///    internal broadcast channel
async fn handle_connection(
    ws_stream: &mut WebSocketStream<TlsStream<TcpStream>>,
    mut state_updates_rx: broadcast::Receiver<SmbDropState>,
    last_state: Arc<Mutex<EventSystemState>>,
    agent_id: AgentId,
) -> Result<(), HandlerError> {
    loop {
        select! {
            msg = ws_stream.next() => {
                let msg = if let Some(msg) = msg { msg? } else { break };
                if let Ok(msg) = msg.into_text() {
                    info!("Got ws message: {}", msg);
                    handle_ws_message(&msg, ws_stream, Arc::clone(&last_state), &agent_id)
                        .await?;
                } else {
                    warn!("received non-text ws message or conversion failed");
                }
            },
            maybe_state = state_updates_rx.recv() => {
                let new_state = maybe_state.map_err(|_| HandlerError::Disconnected)?;
                // compose the update of shares and users with connected shares:
                info!("propagating state update from channel: {new_state:?}");
                let comp_state = EventSystemState {
                    inner_state: new_state,
                    connected_agents: last_state.lock().await.connected_agents.clone(),
                };
                handle_state_update(&comp_state, ws_stream, &agent_id).await?;
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

async fn handle_authenticated_connection(
    peer: SocketAddr,
    version: &str,
    mut ws_stream: WebSocketStream<TlsStream<TcpStream>>,
    claims: Claims,
    last_state: Arc<Mutex<EventSystemState>>,
    state_updates_rx: broadcast::Receiver<SmbDropState>,
    state_updates_tx: broadcast::Sender<SmbDropState>,
) -> Result<(), HandlerError> {
    info!(
        "connection {peer} authenticated with token from {} version {version}",
        claims.sub
    );

    let agent_id = AgentId::from_uri(&claims.sub)?;

    // update connected agents and broadcast a state update
    {
        let mut state_unlocked = last_state.lock().await;
        state_unlocked.connected_agents.insert(agent_id.clone());
        state_updates_tx
            .send(state_unlocked.inner_state.clone())
            .map_err(|_| HandlerError::Disconnected)?;
    }

    let conn_last_state = Arc::clone(&last_state);

    if let Err(e) = handle_connection(
        &mut ws_stream,
        state_updates_rx,
        conn_last_state,
        agent_id.clone(),
    )
    .await
    {
        match e {
            HandlerError::WsError(tungstenite::Error::ConnectionClosed)
            | HandlerError::WsError(tungstenite::Error::Protocol(_))
            | HandlerError::WsError(tungstenite::Error::Utf8(_)) => (),
            err => error!("Error handling connection: {err}"),
        }
    }

    // NOTE: be careful not to put anything that can panic or return between the
    // update at the top and this one, so the state is properly updated on
    // errors, too
    // TODO: if possible replace with a guard that removes the agent on Drop
    {
        let mut state_unlocked = last_state.lock().await;
        state_unlocked.connected_agents.remove(&agent_id);
        state_updates_tx
            .send(state_unlocked.inner_state.clone())
            .map_err(|_| HandlerError::Disconnected)?;
    }

    Ok(())
}

async fn authenticate_agent_connection(
    peer: SocketAddr,
    stream: TlsStream<TcpStream>,
    token_verifier: &LocalTokenVerifier,
    last_state: Arc<Mutex<EventSystemState>>,
    state_updates_rx: broadcast::Receiver<SmbDropState>,
    state_updates_tx: broadcast::Sender<SmbDropState>,
) -> Result<(), HandlerError> {
    let mut ws_stream = accept_async(stream).await?;

    // the first message must be the authentication, otherwise we err out:
    let auth_msg = ws_stream.next().await;
    match auth_msg {
        Some(res) => {
            let msg = res?;
            if let Ok(msg) = msg.into_text() {
                let msg: EventChannelInitialMessages =
                    serde_json::from_str(&msg).map_err(|e| HandlerError::Auth(e.to_string()))?;

                match msg {
                    EventChannelInitialMessages::Login { token, version } => {
                        debug!("connection attempt; client version {version}");
                        if let Some(claims) = token_verifier.verify(&token) {
                            handle_authenticated_connection(
                                peer,
                                &version,
                                ws_stream,
                                claims,
                                last_state,
                                state_updates_rx,
                                state_updates_tx,
                            )
                            .await?;
                            Ok(())
                        } else {
                            Err(HandlerError::Auth("token verification failed".to_owned()))
                        }
                    }
                }
            } else {
                Err(HandlerError::Auth("first message must be text".to_owned()))
            }
        }
        None => Err(HandlerError::Auth(
            "first message could not be received".to_owned(),
        )),
    }
}

async fn accept_agent_connection(
    peer: SocketAddr,
    stream: TcpStream,
    acceptor: TlsAcceptor,
    last_state: Arc<Mutex<EventSystemState>>,
    state_updates_rx: broadcast::Receiver<SmbDropState>,
    state_updates_tx: broadcast::Sender<SmbDropState>,
    token_verifier: LocalTokenVerifier,
) -> Result<(), AcceptError> {
    let stream: TlsStream<TcpStream> = acceptor.accept(stream).await?;

    info!("connection from agent @ {peer}");

    if let Err(e) = authenticate_agent_connection(
        peer,
        stream,
        &token_verifier,
        last_state,
        state_updates_rx,
        state_updates_tx,
    )
    .await
    {
        error!("error in agent connection: {e:?}");
    } else {
        info!("finished handling connection from {peer}")
    }

    Ok(())
}

/// Start a websocket server that informs agents about currently active ingest
/// shares.
///
/// The given certificate must be enrolled via the CA as a server certificate.
///
/// Callers should pass in the initial state as `state`, and then use the
/// returned broadcast `Sender` to update the state when it changes.
pub async fn start_event_server(
    listen_addr: &str,
    tls_config: ServerConfig,
    secret_key: &[u8],
    state: &SmbDropState,
) -> Result<(JoinHandle<()>, broadcast::Sender<SmbDropState>, u16), ServerStartError> {
    let state = EventSystemState {
        inner_state: state.clone(),
        connected_agents: HashSet::new(),
    };
    let (tx, mut rx) = broadcast::channel::<SmbDropState>(16);
    let token_verifier = LocalTokenVerifier::new(secret_key);

    // the state we have seen last, which will be sent to newly connecting agents:
    let last_state = Arc::new(Mutex::new(state));

    // and a coroutine to keep it up to date via the messages from the broadcast channel:
    let wr_last_state = Arc::clone(&last_state);
    tokio::spawn(async move {
        loop {
            let maybe_state = rx.recv().await;
            if let Ok(state) = maybe_state {
                wr_last_state.lock().await.inner_state = state;
            } else if let Err(RecvError::Closed) = maybe_state {
                break;
            }
        }
    });

    let result_tx = tx.clone();
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

    let moved_last_state = Arc::clone(&last_state);
    let local_token_verifier = token_verifier.clone();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            if let Ok(peer) = stream.peer_addr() {
                let acc = acceptor.clone();
                tokio::spawn(accept_agent_connection(
                    peer,
                    stream,
                    acc,
                    Arc::clone(&moved_last_state),
                    tx.subscribe(),
                    tx.clone(),
                    local_token_verifier.clone(),
                ));
            } else {
                error!("no peer addr, dropping client");
            }
        }
    });

    Ok((task, result_tx, port))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use std::{
        collections::HashMap,
        sync::Arc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use futures_util::{
        SinkExt, StreamExt,
        stream::{SplitSink, SplitStream},
    };
    use modama_common::{
        agent::AgentId,
        ca::Ca,
        messages::{EventChannelInitialMessages, EventChannelMessage, EventChannelRequests, Share},
        test_helpers::MockTokenProvider,
        tokens::{Claims, LocalTokenSigner, TokenProvider},
        version::modama_git_version,
    };
    use rustls::{
        ClientConfig, RootCertStore, ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
        server::WebPkiClientVerifier,
    };
    use tokio::{net, sync::broadcast::Sender, task::JoinHandle, time::timeout};
    use tokio_tungstenite::{
        Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    };
    use tungstenite::Message;

    use crate::{events::start_event_server, share_manager::SmbDropState};

    struct CertsForTesting {
        pub server_config: ServerConfig,
        pub server_secret: Vec<u8>,
        pub client_config_1: ClientConfig,
        pub client_config_2: ClientConfig,
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
            let client_cert_verifier = WebPkiClientVerifier::builder(roots.clone().into())
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

            CertsForTesting {
                server_config,
                client_config_1,
                client_config_2,
                server_secret: server_pk_der.secret_der().to_vec(),
            }
        }
    }

    struct TestEventServer {
        pub ws_read: SplitStream<WebSocketStream<MaybeTlsStream<net::TcpStream>>>,
        pub ws_write: SplitSink<WebSocketStream<MaybeTlsStream<net::TcpStream>>, Message>,
        pub event_queue: Sender<SmbDropState>,
        pub ws_url: String,
        pub certs: CertsForTesting,
        pub handle: JoinHandle<()>,
        pub token_provider: Arc<dyn TokenProvider>,
    }

    async fn make_test_event_server() -> TestEventServer {
        let certs = CertsForTesting::generate();
        let state = SmbDropState {
            shares: HashMap::new(),
            users: HashMap::new(),
        };
        let listen_addr = "localhost:0";
        let secret = &certs.server_secret;
        let (task, event_server_queue, port) =
            start_event_server(listen_addr, certs.server_config.clone(), secret, &state)
                .await
                .unwrap();

        let tls_config = Arc::new(certs.client_config_1.clone());
        let ws_url = format!("wss://localhost:{port}");
        let (conn, _response) = connect_async_tls_with_config(
            ws_url.clone(),
            None,
            false,
            Some(Connector::Rustls(Arc::clone(&tls_config))),
        )
        .await
        .unwrap();

        let (ws_write, ws_read) = conn.split();

        let token_signer = LocalTokenSigner::new(&certs.server_secret);

        let token_provider: Arc<dyn TokenProvider + 'static> = Arc::new(MockTokenProvider {
            signer: token_signer,
            claims: Claims {
                sub: "x-modama-agent://test_resource#test_agent".to_owned(),
                exp: (SystemTime::now() + Duration::from_secs(1000))
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
        });

        TestEventServer {
            ws_read,
            ws_write,
            event_queue: event_server_queue,
            ws_url,
            certs,
            token_provider,
            handle: task,
        }
    }

    #[tokio::test]
    async fn test_websocket_api_initial_message() {
        let mut server = make_test_event_server().await;

        let sign_response = server.token_provider.fetch_token().await.unwrap();
        let login_msg = serde_json::to_string(&EventChannelInitialMessages::Login {
            token: sign_response.token,
            version: modama_git_version().to_owned(),
        })
        .unwrap();
        server
            .ws_write
            .send(Message::Text(login_msg.into()))
            .await
            .unwrap();

        let greeting = timeout(Duration::from_millis(100), server.ws_read.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let msg: EventChannelMessage =
            serde_json::from_str(&greeting.into_text().unwrap()).unwrap();

        println!("{msg:?}");

        assert_eq!(
            msg,
            EventChannelMessage::UpdateShareList {
                shares: vec![],
                foreground_share: None,
                connected_agents: vec![AgentId {
                    resource: "test_resource".to_owned(),
                    agent: "test_agent".to_owned(),
                    san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                }],
            }
        );

        // no other messages:
        let res = timeout(Duration::from_millis(100), server.ws_read.next()).await;
        assert!(res.is_err());

        server.handle.abort();
    }

    #[tokio::test]
    async fn test_websocket_api_second_agent() {
        let mut server = make_test_event_server().await;

        let sign_response = Arc::clone(&server.token_provider)
            .fetch_token()
            .await
            .unwrap();
        let login_msg = serde_json::to_string(&EventChannelInitialMessages::Login {
            token: sign_response.token,
            version: modama_git_version().to_owned(),
        })
        .unwrap();
        server
            .ws_write
            .send(Message::Text(login_msg.into()))
            .await
            .unwrap();

        let greeting = timeout(Duration::from_millis(100), server.ws_read.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let msg: EventChannelMessage =
            serde_json::from_str(&greeting.into_text().unwrap()).unwrap();

        println!("{msg:?}");

        assert_eq!(
            msg,
            EventChannelMessage::UpdateShareList {
                shares: vec![],
                foreground_share: None,
                connected_agents: vec![AgentId {
                    resource: "test_resource".to_owned(),
                    agent: "test_agent".to_owned(),
                    san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                }],
            }
        );

        // no other messages:
        let res = timeout(Duration::from_millis(100), server.ws_read.next()).await;
        assert!(res.is_err());

        {
            let tls_config_2 = Arc::new(server.certs.client_config_2.clone());
            // now another agent connects:
            let (mut conn, _response) = connect_async_tls_with_config(
                server.ws_url.clone(),
                None,
                false,
                Some(Connector::Rustls(Arc::clone(&tls_config_2))),
            )
            .await
            .unwrap();

            let token_signer = LocalTokenSigner::new(&server.certs.server_secret);
            let second_token_provider: Arc<dyn TokenProvider + 'static> =
                Arc::new(MockTokenProvider {
                    signer: token_signer,
                    claims: Claims {
                        sub: "x-modama-agent://test_resource#test_agent_2".to_owned(),
                        exp: (SystemTime::now() + Duration::from_secs(1000))
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    },
                });

            let sign_response_2 = Arc::clone(&second_token_provider)
                .fetch_token()
                .await
                .unwrap();
            let login_msg_2 = serde_json::to_string(&EventChannelInitialMessages::Login {
                token: sign_response_2.token,
                version: modama_git_version().to_owned(),
            })
            .unwrap();
            conn.send(Message::Text(login_msg_2.into())).await.unwrap();

            // and we get an updated list of connected agents:
            let update_msg = timeout(Duration::from_millis(100), server.ws_read.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();

            let update: EventChannelMessage =
                serde_json::from_str(&update_msg.into_text().unwrap()).unwrap();

            println!("{update:?}");

            // no other messages:
            let res = timeout(Duration::from_millis(100), server.ws_read.next()).await;
            assert!(res.is_err());

            // now compare the actual messages:
            assert_eq!(
                update,
                EventChannelMessage::UpdateShareList {
                    shares: vec![],
                    foreground_share: None,
                    connected_agents: vec![
                        AgentId {
                            resource: "test_resource".to_owned(),
                            agent: "test_agent".to_owned(),
                            san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                        },
                        AgentId {
                            resource: "test_resource".to_owned(),
                            agent: "test_agent_2".to_owned(),
                            san_encoded: "x-modama-agent://test_resource#test_agent_2".to_owned()
                        },
                    ],
                }
            );

            conn.close(None).await.unwrap();
        }

        let update_msg = timeout(Duration::from_millis(100), server.ws_read.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let msg: EventChannelMessage =
            serde_json::from_str(&update_msg.into_text().unwrap()).unwrap();

        println!("{msg:?}");

        // second agent is gone:
        assert_eq!(
            msg,
            EventChannelMessage::UpdateShareList {
                shares: vec![],
                foreground_share: None,
                connected_agents: vec![AgentId {
                    resource: "test_resource".to_owned(),
                    agent: "test_agent".to_owned(),
                    san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                }],
            }
        );

        // no other messages:
        let res = timeout(Duration::from_millis(100), server.ws_read.next()).await;
        assert!(res.is_err());
        server.handle.abort();
    }

    #[tokio::test]
    async fn test_websocket_api_request_share_update() {
        let mut server = make_test_event_server().await;

        let sign_response = server.token_provider.fetch_token().await.unwrap();
        let login_msg = serde_json::to_string(&EventChannelInitialMessages::Login {
            token: sign_response.token,
            version: modama_git_version().to_owned(),
        })
        .unwrap();
        server
            .ws_write
            .send(Message::Text(login_msg.into()))
            .await
            .unwrap();

        let greeting = timeout(Duration::from_millis(100), server.ws_read.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let msg: EventChannelMessage =
            serde_json::from_str(&greeting.into_text().unwrap()).unwrap();
        println!("{msg:?}");
        assert_eq!(
            msg,
            EventChannelMessage::UpdateShareList {
                shares: vec![],
                foreground_share: None,
                connected_agents: vec![AgentId {
                    resource: "test_resource".to_owned(),
                    agent: "test_agent".to_owned(),
                    san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                }],
            }
        );

        // no other messages:
        let res = timeout(Duration::from_millis(100), server.ws_read.next()).await;
        assert!(res.is_err());

        // now we request an update:
        let update_msg = EventChannelRequests::ShareListUpdate;
        server
            .ws_write
            .send(Message::Text(
                serde_json::to_string(&update_msg).unwrap().into(),
            ))
            .await
            .unwrap();

        let update_msg_resp = timeout(Duration::from_millis(100), server.ws_read.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        // we get the shares and agents again:
        let msg: EventChannelMessage =
            serde_json::from_str(&update_msg_resp.into_text().unwrap()).unwrap();
        println!("{msg:?}");
        assert_eq!(
            msg,
            EventChannelMessage::UpdateShareList {
                shares: vec![],
                foreground_share: None,
                connected_agents: vec![AgentId {
                    resource: "test_resource".to_owned(),
                    agent: "test_agent".to_owned(),
                    san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                }],
            }
        );
        server.handle.abort();
    }

    #[tokio::test]
    async fn test_websocket_api_share_added() {
        let mut server = make_test_event_server().await;

        let sign_response = server.token_provider.fetch_token().await.unwrap();
        let login_msg = serde_json::to_string(&EventChannelInitialMessages::Login {
            token: sign_response.token,
            version: modama_git_version().to_owned(),
        })
        .unwrap();
        server
            .ws_write
            .send(Message::Text(login_msg.into()))
            .await
            .unwrap();

        let greeting = timeout(Duration::from_millis(100), server.ws_read.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let msg: EventChannelMessage =
            serde_json::from_str(&greeting.into_text().unwrap()).unwrap();
        println!("{msg:?}");
        assert_eq!(
            msg,
            EventChannelMessage::UpdateShareList {
                shares: vec![],
                foreground_share: None,
                connected_agents: vec![AgentId {
                    resource: "test_resource".to_owned(),
                    agent: "test_agent".to_owned(),
                    san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                }],
            }
        );

        // no other messages:
        let res = timeout(Duration::from_millis(100), server.ws_read.next()).await;
        assert!(res.is_err());

        let shares = HashMap::new();
        let users = HashMap::new();
        let mut state = SmbDropState { shares, users };

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
        };

        state
            .get_or_create_user(1001, share_id.clone())
            .await
            .unwrap();
        state.add_share(share.clone()).await.unwrap();
        server.event_queue.send(state).unwrap();

        // the share manager informs us about the change:
        let state_update_msg = timeout(Duration::from_millis(100), server.ws_read.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        // the new share is in the list of shares:
        let msg: EventChannelMessage =
            serde_json::from_str(&state_update_msg.into_text().unwrap()).unwrap();
        println!("{msg:?}");
        assert_eq!(
            msg,
            EventChannelMessage::UpdateShareList {
                shares: vec![share.clone()],
                foreground_share: Some(share),
                connected_agents: vec![AgentId {
                    resource: "test_resource".to_owned(),
                    agent: "test_agent".to_owned(),
                    san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                }],
            }
        );
        server.handle.abort();
    }
}
