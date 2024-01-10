use std::{
    fs::File,
    io::{self, BufReader},
    path::{Path, PathBuf},
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

    #[error("key or certificate not found in the PEM file `{0}`")]
    KeyNotFound(PathBuf),

    #[error("TLS error")]
    TlsError(#[from] rustls::Error),
}

pub fn get_root_cert_store(ca_cert: &Path) -> Result<RootCertStore, CertHelperError> {
    let mut root_cert_store = RootCertStore::empty();

    let ca_certs = File::open(ca_cert).map_err(|e| CertHelperError::IoError {
        source: e,
        path: ca_cert.to_owned(),
    })?;
    for cert in certs(&mut BufReader::new(ca_certs)) {
        root_cert_store.add(cert.map_err(|e| CertHelperError::IoError {
            source: e,
            path: ca_cert.to_owned(),
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
