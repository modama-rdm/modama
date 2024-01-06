use std::{fs::File, io::BufReader, path::Path, sync::Arc, net::SocketAddr};

use log::warn;
use rustls_pemfile::{certs, private_key};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    sync::{broadcast::{self, error::RecvError}, Mutex},
};
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer},
        server::{VerifierBuilderError, WebPkiClientVerifier},
        RootCertStore, ServerConfig,
    },
    TlsAcceptor,
};

use crate::share_manager::SmbDropState;

#[derive(thiserror::Error, Debug)]
pub enum ServerStartError {
    #[error("listen failed")]
    ListenError,

    #[error("certificate error")]
    CertError(Box<dyn std::error::Error>),
}

impl From<std::io::Error> for ServerStartError {
    fn from(value: std::io::Error) -> Self {
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

fn get_root_cert_store(ca_cert: &Path) -> Result<RootCertStore, ServerStartError> {
    let mut root_cert_store = RootCertStore::empty();

    let ca_certs = File::open(ca_cert)?;
    for cert in certs(&mut BufReader::new(ca_certs)) {
        root_cert_store.add(cert?)?;
    }

    Ok(root_cert_store)
}

fn get_server_chain(chain: &Path) -> Result<Vec<CertificateDer<'static>>, ServerStartError> {
    let cert_file = File::open(chain)?;

    // FIXME: fail instead of flattening
    Ok(certs(&mut BufReader::new(cert_file)).flatten().collect())
}

fn get_server_key(privkey: &Path) -> Result<PrivateKeyDer<'static>, ServerStartError> {
    let key_file = File::open(privkey)?;
    let pk = private_key(&mut BufReader::new(key_file))?.expect("private key file contains a key");
    Ok(pk)
}

fn load_tls_config(
    ca_cert: &Path,
    chain: &Path,
    privkey: &Path,
) -> Result<ServerConfig, ServerStartError> {
    let roots = get_root_cert_store(ca_cert)?;
    let client_cert_verifier = WebPkiClientVerifier::builder(roots.into()).build()?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(client_cert_verifier)
        .with_single_cert(get_server_chain(chain)?, get_server_key(privkey)?)?;

    Ok(config)
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
    ca_cert: &Path,
    chain: &Path,
    privkey: &Path,
    state: &SmbDropState,
) -> Result<broadcast::Sender<SmbDropState>, ServerStartError> {
    let config = load_tls_config(ca_cert, chain, privkey)?;
    let (tx, mut rx) = broadcast::channel(16);

    // the state we have seen last, which will be sent to newly connecting agents:
    let last_state = Arc::new(Mutex::new(state.clone()));

    // and a coroutine to keep it up to date via the messages from the broadcast channel:
    tokio::spawn(async move {
        loop {
            let maybe_state = rx.recv().await;
            if let Ok(state) = maybe_state {
                *last_state.lock().await = state;
            } else if let Err(RecvError::Closed) = maybe_state {
                break
            }
        }
    });

    let result_tx = tx.clone();
    let listener = TcpListener::bind(&listen_addr).await.expect("Can't listen");
    let acceptor = TlsAcceptor::from(Arc::new(config));

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let peer = stream.peer_addr().expect("should have a peer address");
            let acc = acceptor.clone();
            tokio::spawn(accept_agent_connection(peer, stream, acc, tx.subscribe()));
        }
    });

    Ok(result_tx)
}

async fn accept_agent_connection(
    peer: SocketAddr,
    stream: TcpStream,
    acceptor: TlsAcceptor,
    subscribe: broadcast::Receiver<SmbDropState>,
) {
    let stream = acceptor.accept(stream).await.unwrap();
    let (_, server_connection) = stream.get_ref();

    let peer_certs = server_connection.peer_certificates().unwrap();
    let peer_cert = peer_certs.get(0).unwrap();

    let agent_id = AgentId::from_cert(peer_cert).unwrap();

    info!("agent id: {agent_id:?}");

    if let Err(e) = handle_connection(peer, stream.into()).await {
        match e {
            tungstenite::Error::ConnectionClosed
            | tungstenite::Error::Protocol(_)
            | tungstenite::Error::Utf8 => (),
            err => error!("Error handling connection: {err}"),
        }
    }
}
