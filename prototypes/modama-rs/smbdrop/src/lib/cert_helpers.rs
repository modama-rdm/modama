use std::path::Path;

use modama_common::{
    agent::AgentId,
    cert_helpers::{get_cert_chain, get_private_key, get_root_cert_store},
};
use rustls::{server::WebPkiClientVerifier, ServerConfig};
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;

use crate::events::ServerStartError;

pub fn load_tls_config(
    ca_cert: &Path,
    chain: &Path,
    privkey: &Path,
    require_client_cert: bool,
) -> Result<ServerConfig, ServerStartError> {
    let roots = get_root_cert_store(ca_cert)?;
    let client_cert_verifier = if require_client_cert {
        WebPkiClientVerifier::builder(roots.into()).build()?
    } else {
        WebPkiClientVerifier::builder(roots.into())
            .allow_unauthenticated()
            .build()?
    };

    let config = ServerConfig::builder()
        .with_client_cert_verifier(client_cert_verifier)
        .with_single_cert(get_cert_chain(chain)?, get_private_key(privkey)?)?;

    Ok(config)
}

pub fn agent_id_from_stream(stream: &TlsStream<TcpStream>) -> Option<AgentId> {
    let (_, server_connection) = stream.get_ref();
    let peer_certs = server_connection.peer_certificates()?;
    let peer_cert = peer_certs.get(0)?;
    AgentId::from_cert(peer_cert).ok()
}
