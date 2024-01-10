use std::{fs::File, io::BufReader, path::Path};

use modama_common::agent::AgentId;
use rustls_pemfile::{certs, private_key};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    server::WebPkiClientVerifier,
    ServerConfig,
    RootCertStore,
};
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;

use crate::events::ServerStartError;

pub fn get_root_cert_store(ca_cert: &Path) -> Result<RootCertStore, ServerStartError> {
    let mut root_cert_store = RootCertStore::empty();

    let ca_certs = File::open(ca_cert).map_err(|e| ServerStartError::IoError {
        source: e,
        path: ca_cert.to_owned(),
    })?;
    for cert in certs(&mut BufReader::new(ca_certs)) {
        root_cert_store.add(cert?)?;
    }

    Ok(root_cert_store)
}

pub fn get_server_chain(chain: &Path) -> Result<Vec<CertificateDer<'static>>, ServerStartError> {
    let cert_file = File::open(chain).map_err(|e| ServerStartError::IoError {
        source: e,
        path: chain.to_owned(),
    })?;

    // FIXME: fail instead of flattening
    Ok(certs(&mut BufReader::new(cert_file)).flatten().collect())
}

pub fn get_private_key(privkey: &Path) -> Result<PrivateKeyDer<'static>, ServerStartError> {
    let key_file = File::open(privkey).map_err(|e| ServerStartError::IoError {
        source: e,
        path: privkey.to_owned(),
    })?;
    let pk =
        private_key(&mut BufReader::new(key_file))?.expect("private key file should contain a key");
    Ok(pk)
}

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
        WebPkiClientVerifier::builder(roots.into()).allow_unauthenticated().build()?
    };

    let config = ServerConfig::builder()
        .with_client_cert_verifier(client_cert_verifier)
        .with_single_cert(get_server_chain(chain)?, get_private_key(privkey)?)?;

    Ok(config)
}

pub fn agent_id_from_stream(stream: &TlsStream<TcpStream>) -> Option<AgentId> {
    let (_, server_connection) = stream.get_ref();
    let peer_certs = server_connection.peer_certificates()?;
    let peer_cert = peer_certs.get(0)?;
    AgentId::from_cert(peer_cert).ok()
}
