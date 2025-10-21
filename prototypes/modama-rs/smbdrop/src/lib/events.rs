use std::{collections::HashSet, io, net::SocketAddr, path::PathBuf, sync::Arc};

use crate::share_manager::SmbDropState;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use modama_common::{
    agent::{AgentId, ClientCertError},
    cert_helpers::CertHelperError,
    messages::{EventChannelMessage, EventChannelRequests, Share},
};
use rustls::ServerConfig;
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    sync::{
        broadcast::{self, error::RecvError},
        Mutex,
    },
    task::JoinHandle,
};
use tokio_rustls::{rustls::server::VerifierBuilderError, server::TlsStream, TlsAcceptor};
use tokio_tungstenite::{accept_async, WebSocketStream};

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
        // explicit request of the current share list
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
    let msg = EventChannelMessage::UpdateShareList {
        shares: shares.clone(),
        foreground_share: shares.iter().last().cloned(),
        connected_agents: state.connected_agents.iter().cloned().collect(),
    };
    msg
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
    stream: TlsStream<TcpStream>,
    mut state_updates_rx: broadcast::Receiver<SmbDropState>,
    last_state: Arc<Mutex<EventSystemState>>,
    agent_id: AgentId,
) -> Result<(), HandlerError> {
    let mut ws_stream = accept_async(stream).await?;

    loop {
        select! {
            msg = ws_stream.next() => {
                let msg = if let Some(msg) = msg { msg? } else { break };
                if let Ok(msg) = msg.into_text() {
                    info!("Got ws message: {}", msg);
                    handle_ws_message(&msg, &mut ws_stream, Arc::clone(&last_state), &agent_id)
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
                handle_state_update(&comp_state, &mut ws_stream, &agent_id).await?;
            }
        }
    }

    Ok(())
}

#[derive(thiserror::Error, Debug)]
enum AcceptError {
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Extract certificate")]
    NoCertificate,

    #[error("Validate agent ID: {0}")]
    ClientCert(#[from] ClientCertError),

    #[error("channel disconnected")]
    Disconnected,
}

async fn accept_agent_connection(
    peer: SocketAddr,
    stream: TcpStream,
    acceptor: TlsAcceptor,
    last_state: Arc<Mutex<EventSystemState>>,
    state_updates_rx: broadcast::Receiver<SmbDropState>,
    state_updates_tx: broadcast::Sender<SmbDropState>,
) -> Result<(), AcceptError> {
    let stream = acceptor.accept(stream).await?;
    let (_, server_connection) = stream.get_ref();

    let peer_certs = server_connection
        .peer_certificates()
        .ok_or(AcceptError::NoCertificate)?;
    let peer_cert = peer_certs.first().ok_or(AcceptError::NoCertificate)?;

    let agent_id = AgentId::from_cert(peer_cert)?;

    info!("connection from agent id: {agent_id:?} @ {peer}");

    // update connected agents and broadcast a state update
    {
        let mut state_unlocked = last_state.lock().await;
        state_unlocked.connected_agents.insert(agent_id.clone());
        state_updates_tx
            .send(state_unlocked.inner_state.clone())
            .map_err(|_| AcceptError::Disconnected)?;
    }

    let conn_last_state = Arc::clone(&last_state);

    if let Err(e) =
        handle_connection(stream, state_updates_rx, conn_last_state, agent_id.clone()).await
    {
        match e {
            HandlerError::WsError(tungstenite::Error::ConnectionClosed)
            | HandlerError::WsError(tungstenite::Error::Protocol(_))
            | HandlerError::WsError(tungstenite::Error::Utf8(_)) => (),
            err => error!("Error handling connection: {err}"),
        }
    }

    // FIXME: also remove from connected_agents on error (is there such a thing
    // as an async drop guard yet in rust?)
    // update connected agents and broadcast a state update
    {
        let mut state_unlocked = last_state.lock().await;
        state_unlocked.connected_agents.remove(&agent_id);
        state_updates_tx
            .send(state_unlocked.inner_state.clone())
            .map_err(|_| AcceptError::Disconnected)?;
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
    state: &SmbDropState,
) -> Result<(JoinHandle<()>, broadcast::Sender<SmbDropState>, u16), ServerStartError> {
    let state = EventSystemState {
        inner_state: state.clone(),
        connected_agents: HashSet::new(),
    };
    let (tx, mut rx) = broadcast::channel::<SmbDropState>(16);

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
    use std::{collections::HashMap, sync::Arc, time::Duration};

    use futures_util::{
        stream::{SplitSink, SplitStream},
        SinkExt, StreamExt,
    };
    use hyper::client;
    use modama_common::{
        agent::AgentId,
        ca::Ca,
        messages::{EventChannelMessage, EventChannelRequests},
    };
    use rustls::{
        crypto::{self, CryptoProvider},
        pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
        server::WebPkiClientVerifier,
        ClientConfig, RootCertStore, ServerConfig,
    };
    use tokio::{net, sync::broadcast::Sender, time::timeout};
    use tokio_tungstenite::{
        connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
    };
    use tungstenite::Message;

    use crate::{events::start_event_server, share_manager::SmbDropState};

    struct CertsForTesting {
        pub server_config: ServerConfig,
        pub client_config_1: ClientConfig,
        pub client_config_2: ClientConfig,
    }

    impl CertsForTesting {
        fn generate() -> Self {
            let ca = Ca::new::<String>(None).unwrap();
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
                .with_single_cert(server_chain, server_pk_der)
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
            }
        }
    }

    struct TestEventServer {
        pub ws_read: SplitStream<WebSocketStream<MaybeTlsStream<net::TcpStream>>>,
        pub ws_write: SplitSink<WebSocketStream<MaybeTlsStream<net::TcpStream>>, Message>,
        pub event_queue: Sender<SmbDropState>,
        pub ws_url: String,
        pub certs: CertsForTesting,
    }

    async fn make_test_event_server() -> (TestEventServer) {
        // CryptoProvider::get_default().unwrap();
        // CryptoProvider::install_default(crypto::aws_lc_rs::default_provider()).unwrap();
        let certs = CertsForTesting::generate();
        let state = SmbDropState {
            shares: HashMap::new(),
            users: HashMap::new(),
        };
        let listen_addr = "localhost:0";
        let (task, event_server_queue, port) =
            start_event_server(listen_addr, certs.server_config.clone(), &state)
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

        TestEventServer {
            ws_read,
            ws_write,
            event_queue: event_server_queue,
            ws_url,
            certs,
        }
    }

    #[tokio::test]
    async fn test_websocket_api_initial_message() {
        let mut server = make_test_event_server().await;
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
    }

    #[tokio::test]
    async fn test_websocket_api_second_agent() {
        let mut server = make_test_event_server().await;

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

            // and we get an updated list of connected agents:
            let update_msg = timeout(Duration::from_millis(100), server.ws_read.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();

            let update: EventChannelMessage =
                serde_json::from_str(&update_msg.into_text().unwrap()).unwrap();

            println!("{update:?}");

            assert_eq!(
                update,
                EventChannelMessage::UpdateShareList {
                    shares: vec![],
                    foreground_share: None,
                    connected_agents: vec![
                        AgentId {
                            resource: "test_resource".to_owned(),
                            agent: "test_agent_2".to_owned(),
                            san_encoded: "x-modama-agent://test_resource#test_agent_2".to_owned()
                        },
                        AgentId {
                            resource: "test_resource".to_owned(),
                            agent: "test_agent".to_owned(),
                            san_encoded: "x-modama-agent://test_resource#test_agent".to_owned()
                        },
                    ],
                }
            );

            // no other messages:
            let res = timeout(Duration::from_millis(100), server.ws_read.next()).await;
            assert!(res.is_err());

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
    }

    #[tokio::test]
    async fn test_websocket_api_request_share_update() {
        let mut server = make_test_event_server().await;

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
    }
}
