use thiserror::Error;
use x509_parser::{error::X509Error, nom::Err};

#[derive(Error, Debug)]
pub enum ClientCertError {
    #[error("invalid or missing SAN")]
    InvalidSan(Option<String>),

    #[error("error parsing the certificate")]
    X509ParsingError(#[from] Err<X509Error>),

    #[error("x509 error")]
    X509Error(#[from] X509Error),
}
