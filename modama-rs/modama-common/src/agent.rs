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
    #[error("no subject alternative name")]
    MissingSan,

    #[error("subject alternative name is not a URI: {0}")]
    SanNotUri(String),

    #[error(transparent)]
    InvalidURI(#[from] FromURIError),

    #[error("error parsing the certificate: {0}")]
    X509ParsingError(#[from] Err<X509Error>),

    #[error("x509 error: {0}")]
    X509Error(#[from] X509Error),
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum FromURIError {
    #[error("invalid URI in SAN field: {0}")]
    InvalidURI(String),

    #[error("invalid or missing URI parts in {0}")]
    URIParts(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId {
    pub resource: String,
    pub agent: String,
    pub san_encoded: String,
}

impl AgentId {
    pub fn from_uri(uri: &str) -> Result<Self, FromURIError> {
        let uri_parsed =
            Url::from_str(uri).map_err(|_e| FromURIError::InvalidURI(uri.to_owned()))?;

        if uri_parsed.scheme() != "x-modama-agent" {
            return Err(FromURIError::InvalidURI(uri_parsed.to_string()));
        }

        Ok(Self {
            resource: uri_parsed
                .host_str()
                .ok_or_else(|| FromURIError::URIParts(uri.to_owned()))?
                .to_string(),
            agent: uri_parsed
                .fragment()
                .ok_or_else(|| FromURIError::URIParts(uri.to_owned()))?
                .to_string(),
            san_encoded: uri.to_owned(),
        })
    }

    pub fn from_cert(cert: &CertificateDer) -> Result<Self, ClientCertError> {
        let (_rem, client_cert) = X509Certificate::from_der(cert)?;

        let san = client_cert
            .subject_alternative_name()?
            .ok_or(ClientCertError::MissingSan)?;
        let name = &san
            .value
            .general_names
            .first()
            .ok_or(ClientCertError::MissingSan)?;
        debug!("client SAN: {name}");
        match name {
            x509_parser::extensions::GeneralName::URI(u) => Ok(Self::from_uri(u)?),
            _ => Err(ClientCertError::SanNotUri(name.to_string())),
        }
    }
}
