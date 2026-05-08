use std::{
    fs::File,
    io::{self, BufReader},
    path::{Path, PathBuf},
};

use rustls::{
    server::{VerifierBuilderError, WebPkiClientVerifier},
    ServerConfig,
};
use rustls_pemfile::{certs, private_key};
use tokio_rustls::rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    RootCertStore,
};

#[derive(thiserror::Error, Debug)]
pub enum CertHelperError {
    #[error("error reading `{path}`: {source}")]
    IoError { source: io::Error, path: PathBuf },

    #[error("tls server config: {0}")]
    TlsServerConfig(#[from] VerifierBuilderError),

    #[error("key or certificate not found in the PEM file `{0}`")]
    KeyNotFound(PathBuf),

    #[error("TLS error")]
    TlsError(#[from] rustls::Error),
}

pub fn get_root_cert_store_from_file(path: &Path) -> Result<RootCertStore, CertHelperError> {
    let certs_file = File::open(path).map_err(|e| CertHelperError::IoError {
        source: e,
        path: path.to_owned(),
    })?;

    get_root_cert_store(&mut BufReader::new(certs_file), path)
}

pub fn get_root_cert_store(
    mut reader: &mut impl std::io::BufRead,
    source_path: &Path,
) -> Result<RootCertStore, CertHelperError> {
    let mut root_cert_store = RootCertStore::empty();

    for cert in certs(&mut reader) {
        root_cert_store.add(cert.map_err(|e| CertHelperError::IoError {
            source: e,
            path: source_path.to_owned(),
        })?)?;
    }

    Ok(root_cert_store)
}

pub fn get_cert_chain(chain: &Path) -> Result<Vec<CertificateDer<'static>>, CertHelperError> {
    let cert_file = File::open(chain).map_err(|e| CertHelperError::IoError {
        source: e,
        path: chain.to_owned(),
    })?;

    let mut chain_certs = Vec::new();
    for cert in certs(&mut BufReader::new(cert_file)) {
        chain_certs.push(cert.map_err(|e| CertHelperError::IoError {
            source: e,
            path: chain.to_owned(),
        })?);
    }

    Ok(chain_certs)
}

pub fn get_private_key(privkey: &Path) -> Result<PrivateKeyDer<'static>, CertHelperError> {
    let key_file = File::open(privkey).map_err(|e| CertHelperError::IoError {
        source: e,
        path: privkey.to_owned(),
    })?;
    let pk = private_key(&mut BufReader::new(key_file))
        .map_err(|e| CertHelperError::IoError {
            source: e,
            path: privkey.to_owned(),
        })?
        .ok_or_else(|| CertHelperError::KeyNotFound(privkey.to_owned()))?;
    Ok(pk)
}

pub fn load_tls_server_config(
    ca_cert: &Path,
    chain: &Path,
    privkey: &Path,
    require_client_cert: bool,
) -> Result<ServerConfig, CertHelperError> {
    let roots = get_root_cert_store_from_file(ca_cert)?;
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
