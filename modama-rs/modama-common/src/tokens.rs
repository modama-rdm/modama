use std::{
    fmt::Debug,
    fs::File,
    future::Future,
    io::{self, BufRead, BufReader, Read},
    path::Path,
    pin::Pin,
    sync::Arc,
};

use jsonwebtoken::{encode, EncodingKey, Header};
use reqwest::{Certificate, Client, Identity, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{
    agent::{AgentId, FromURIError},
    version::modama_git_version,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenSignResponse {
    /// jwt
    pub token: String,

    /// resource name
    pub resource: String,

    /// agent name
    pub agent_name: String,

    /// san-encoded: x-modama-agent://resource_name#agent_name
    pub agent_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TokenVerifyResponse {
    Invalid,
    VerifiedAgent { agent_id: String },
}

impl TokenVerifyResponse {
    pub fn get_agent_id(&self) -> Option<AgentId> {
        match self {
            TokenVerifyResponse::Invalid => None,
            TokenVerifyResponse::VerifiedAgent { agent_id } => AgentId::from_uri(agent_id).ok(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum TokenError {
    #[error("fetch error: {0}")]
    Fetch(#[from] reqwest::Error),

    #[error("error response code: {0}")]
    ResponseCode(StatusCode),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    IO(#[from] io::Error),

    #[error("No token source found")]
    NoSource,
}

#[derive(Debug)]
pub struct TokenFetcher {
    client: Client,
    sign_service_url: String,
}

impl TokenFetcher {
    pub fn new(
        sign_service_url: &str,
        key_reader: &mut impl BufRead,
        chain_reader: &mut impl BufRead,
        ca_cert_reader: &mut impl BufRead,
    ) -> Result<Self, TokenError> {
        let mut id_pem: Vec<u8> = Vec::new();
        key_reader.read_to_end(&mut id_pem)?;
        chain_reader.read_to_end(&mut id_pem)?;
        let mut ca_pem: Vec<u8> = Vec::new();
        ca_cert_reader.read_to_end(&mut ca_pem)?;
        let root_cert = Certificate::from_pem(&ca_pem)?;

        let identity = Identity::from_pem(&id_pem)?;
        let client = reqwest::ClientBuilder::new()
            .identity(identity)
            .user_agent(format!("modama-agent/{}", modama_git_version()))
            .add_root_certificate(root_cert)
            .build()?;

        Ok(Self {
            client,
            sign_service_url: sign_service_url.to_owned(),
        })
    }

    pub fn from_files(
        sign_service_url: &str,
        key_file: &impl AsRef<Path>,
        chain_file: &impl AsRef<Path>,
        ca_cert_file: &impl AsRef<Path>,
    ) -> Result<Self, TokenError> {
        let mut key_reader = BufReader::new(File::open(key_file)?);
        let mut chain_reader = BufReader::new(File::open(chain_file)?);
        let mut ca_cert_reader = BufReader::new(File::open(ca_cert_file)?);
        Self::new(
            sign_service_url,
            &mut key_reader,
            &mut chain_reader,
            &mut ca_cert_reader,
        )
    }

    pub async fn fetch_token(&self) -> Result<TokenSignResponse, TokenError> {
        let resp = self.client.put(&self.sign_service_url).send().await?;
        if resp.status() == StatusCode::OK {
            let text = resp.text().await?;
            Ok(serde_json::from_str(&text)?)
        } else {
            Err(TokenError::ResponseCode(resp.status()))
        }
    }
}

// Trait to abstract token fetching so tests can provide a stub implementation.
pub type BoxFutureToken =
    Pin<Box<dyn Future<Output = Result<TokenSignResponse, TokenError>> + Send>>;

pub trait TokenProvider: Send + Sync + Debug {
    fn fetch_token(self: Arc<Self>) -> BoxFutureToken;
}

impl TokenProvider for TokenFetcher {
    fn fetch_token(self: Arc<Self>) -> BoxFutureToken {
        Box::pin(async move { TokenFetcher::fetch_token(&self).await })
    }
}

pub struct TokenVerifier {
    client: Client,
    verify_service_url: String,
}

impl TokenVerifier {
    pub fn new(verify_service_url: &str, ca_cert_file: &Path) -> Result<Self, TokenError> {
        let mut ca_certs = File::open(ca_cert_file)?;
        let mut ca_pem: Vec<u8> = Vec::new();
        ca_certs.read_to_end(&mut ca_pem)?;
        let root_cert = Certificate::from_pem(&ca_pem)?;

        Ok(Self {
            client: reqwest::ClientBuilder::new()
                .add_root_certificate(root_cert)
                .user_agent(format!("modama-agent/{}", modama_git_version()))
                .build()?,
            verify_service_url: verify_service_url.to_owned(),
        })
    }

    pub async fn verify(&self, token: &str) -> Result<TokenVerifyResponse, TokenError> {
        let resp = self
            .client
            .put(&self.verify_service_url)
            .body(token.to_owned())
            .send()
            .await?;
        let resp: TokenVerifyResponse = resp.json().await?;
        Ok(resp)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SignError {
    #[error("authentication or client certificate failure")]
    Auth,

    #[error("could not decode claim: {0}")]
    ClaimDecode(#[from] FromURIError),

    #[error("JWT encoding error")]
    Encoding,
}

#[derive(Clone)]
pub struct LocalTokenSigner {
    secret: Vec<u8>,
}

impl Debug for LocalTokenSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTokenSigner")
            .field("secret", &"[redacted]".to_owned())
            .finish()
    }
}

impl LocalTokenSigner {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }

    pub fn sign(&self, claims: &Claims) -> Result<TokenSignResponse, SignError> {
        if let Ok(jwt) = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        ) {
            let agent_id = AgentId::from_uri(&claims.sub)?;
            Ok(TokenSignResponse {
                token: jwt,
                resource: agent_id.resource,
                agent_name: agent_id.agent,
                agent_uri: agent_id.san_encoded,
            })
        } else {
            Err(SignError::Encoding)
        }
    }
}
