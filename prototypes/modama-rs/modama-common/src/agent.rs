use std::str::FromStr;

use log::debug;
use serde::{Deserialize, Serialize};
use tokio_rustls::rustls::pki_types::CertificateDer;
use url::Url;
use x509_parser::{
    certificate::X509Certificate, error::X509Error, nom::Err, oid_registry::asn1_rs::FromDer,
};

#[derive(thiserror::Error, Debug)]
pub enum ClientCertError {
    #[error("invalid or missing SAN")]
    InvalidSan(Option<String>),

    #[error("invalid URI in SAN field: {0}")]
    InvalidURI(String),

    #[error("invalid or missing URI parts in {0}")]
    URIParts(String),

    #[error("error parsing the certificate")]
    X509ParsingError(#[from] Err<X509Error>),

    #[error("x509 error")]
    X509Error(#[from] X509Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId {
    pub resource: String,
    pub agent: String,
    pub san_encoded: String,
}

impl AgentId {
    pub fn from_uri(uri: &str) -> Result<Self, ClientCertError> {
        let uri_parsed =
            Url::from_str(uri).map_err(|_e| ClientCertError::InvalidURI(uri.to_owned()))?;

        if uri_parsed.scheme() != "x-modama-agent" {
            return Err(ClientCertError::InvalidURI(uri_parsed.to_string()));
        }

        Ok(Self {
            resource: uri_parsed.host_str().ok_or_else(|| ClientCertError::URIParts(uri.to_owned()))?.to_string(),
            agent: uri_parsed.fragment().ok_or_else(|| ClientCertError::URIParts(uri.to_owned()))?.to_string(),
            san_encoded: uri.to_owned(),
        })
    }

    pub fn from_cert(cert: &CertificateDer) -> Result<Self, ClientCertError> {
        let (_rem, client_cert) = X509Certificate::from_der(cert)?;

        let san = client_cert
            .subject_alternative_name()?
            .ok_or(ClientCertError::InvalidSan(None))?;
        let name = &san
            .value
            .general_names
            .first()
            .ok_or(ClientCertError::InvalidSan(None))?;
        debug!("client SAN: {name}");
        match name {
            x509_parser::extensions::GeneralName::URI(u) => Self::from_uri(u),
            _ => Err(ClientCertError::InvalidSan(Some(name.to_string()))),
        }
    }
}
