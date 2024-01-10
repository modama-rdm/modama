use std::str::FromStr;

use log::debug;
use thiserror::Error;
use tokio_rustls::rustls::pki_types::CertificateDer;
use url::Url;
use x509_parser::{
    certificate::X509Certificate, error::X509Error, nom::Err, oid_registry::asn1_rs::FromDer,
};

#[derive(Error, Debug)]
pub enum ClientCertError {
    #[error("invalid or missing SAN")]
    InvalidSan(Option<String>),

    #[error("error parsing the certificate")]
    X509ParsingError(#[from] Err<X509Error>),

    #[error("x509 error")]
    X509Error(#[from] X509Error),
}

#[derive(Debug, Clone)]
pub struct AgentId {
    pub resource: String,
    pub agent: String,
    pub san_encoded: Url,
}

impl AgentId {
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

        let (resource, agent, uri) = match name {
            x509_parser::extensions::GeneralName::URI(u) => {
                let uri = Url::from_str(u).unwrap();
                assert!(uri.scheme() == "x-modama-agent");
                (
                    uri.host_str().unwrap().to_string(),
                    uri.fragment().unwrap().to_string(),
                    uri,
                )
            }
            _ => {
                return Err(ClientCertError::InvalidSan(Some(name.to_string())));
            }
        };

        Ok(Self {
            resource,
            agent,
            san_encoded: uri,
        })
    }
}
