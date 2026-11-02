use std::{
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Request, State},
    routing::put,
};
use hyper::{StatusCode, body::Incoming};
use hyper_util::rt::{TokioExecutor, TokioIo};
use jsonwebtoken::{DecodingKey, Validation, decode};
use log::{error, warn};
use modama_common::{
    agent::AgentId,
    cert_helpers::{get_private_key, load_tls_server_config},
    tokens::{Claims, LocalTokenSigner, SignError, TokenSignResponse, TokenVerifyResponse},
};
use tokio::{net::TcpListener, net::TcpStream, task::JoinHandle};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tower_service::Service;

use crate::events::ServerStartError;
#[derive(Clone)]
struct ConnState {
    /// Agent ID - None if client authentication didn't succeed
    pub agent_id: Option<AgentId>,

    /// Private key, DER-encoded
    pub secret: Vec<u8>,

    /// Token verifier
    pub token_verifier: LocalTokenVerifier,
}

const EXPIRY_DURATION: u64 = 3600;

async fn do_sign(state: &ConnState) -> Result<TokenSignResponse, SignError> {
    let agent_id = if let Some(agent_id) = &state.agent_id {
        agent_id.clone()
    } else {
        return Err(SignError::Auth);
    };
    eprintln!("agent id {:?}", agent_id);

    let sub = &agent_id.san_encoded;
    let claims = Claims {
        sub: sub.to_owned(),
        exp: (SystemTime::now() + Duration::from_secs(EXPIRY_DURATION))
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SignError::Encoding)? // something fishy...
            .as_secs(),
    };
    let signer = LocalTokenSigner::new(&state.secret);
    let token = signer.sign(&claims)?;
    Ok(token)
}

async fn sign_jwt(State(state): State<ConnState>) -> Result<Json<TokenSignResponse>, StatusCode> {
    do_sign(&state)
        .await
        .map_err(|e| match e {
            SignError::Auth => StatusCode::FORBIDDEN,
            SignError::Encoding => StatusCode::BAD_REQUEST,
            SignError::ClaimDecode(e) => {
                warn!("could not decode claim: {e}");
                StatusCode::BAD_REQUEST
            }
        })
        .map(Json)
}

async fn verify_jwt(
    State(state): State<ConnState>,
    request: String,
) -> Result<Json<TokenVerifyResponse>, StatusCode> {
    Ok(Json(match state.token_verifier.verify(&request) {
        Some(claims) => TokenVerifyResponse::VerifiedAgent {
            agent_id: claims.sub.clone(),
        },
        None => TokenVerifyResponse::Invalid,
    }))
}

#[derive(Clone)]
pub struct LocalTokenVerifier {
    secret: Vec<u8>,
}

impl LocalTokenVerifier {
    /// Create a local token verifier, the secret key is given DER-encoded
    pub fn new(secret: &[u8]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }

    pub fn verify(&self, token: &str) -> Option<Claims> {
        let key = DecodingKey::from_secret(&self.secret);
        let validation = Validation {
            validate_exp: true, // in case the default _ever_ changes...
            ..Validation::default()
        };
        match decode::<Claims>(token, &key, &validation) {
            Ok(decode_result) => Some(decode_result.claims),
            Err(e) => {
                warn!("error while validating jwt: {e}");
                None
            }
        }
    }
}

pub struct TokenServer {
    listen_port: u16,
    pub handle: JoinHandle<Result<(), ServerStartError>>,
}

impl TokenServer {
    /// Start a HTTP server on the given `listen_addr` with optional client
    /// certificate verification. If a valid certificate for an agent is
    /// presented, a JWT (JSON Web Token) for the agents' resource can be issued.
    ///
    /// Without client certificate, the verification endpoint can be used
    /// to check if a given JWT is valid.
    ///
    /// The server is running in a background task
    pub async fn start(
        listen_addr: &str,
        ca_cert: &Path,
        chain: &Path,
        privkey: &Path,
    ) -> Result<Self, ServerStartError> {
        let config = load_tls_server_config(ca_cert, chain, privkey, false)?;
        let pk = get_private_key(privkey)?;
        let secret = Vec::from(pk.secret_der());

        let listener = TcpListener::bind(&listen_addr).await?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let app = Router::<ConnState>::new()
            .route("/sign/", put(sign_jwt))
            .route("/verify/", put(verify_jwt));

        let listen_port = listener.local_addr()?.port();
        let server_handle = tokio::spawn(async move {
            serve_token_server_with_listener(listener, acceptor, secret, app).await
        });

        Ok(Self {
            listen_port,
            handle: server_handle,
        })
    }

    pub fn port(&self) -> u16 {
        self.listen_port
    }
}

fn agent_id_from_stream(stream: &TlsStream<TcpStream>) -> Option<AgentId> {
    let (_, server_connection) = stream.get_ref();
    let peer_certs = server_connection.peer_certificates()?;
    let peer_cert = peer_certs.first()?;
    AgentId::from_cert(peer_cert).ok()
}

/// Serve requests using an already-bound TcpListener
async fn serve_token_server_with_listener(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    secret: Vec<u8>,
    app: Router<ConnState>,
) -> Result<(), ServerStartError> {
    loop {
        let tower_service = app.clone();
        let tls_acceptor = acceptor.clone();
        let secret = secret.clone();

        let (conn, addr) = listener
            .accept()
            .await
            .map_err(|_| ServerStartError::ListenError)?;

        tokio::spawn(async move {
            let Ok(stream) = tls_acceptor.accept(conn).await else {
                error!("error during handshake from {addr}");
                return;
            };

            let agent_id = agent_id_from_stream(&stream);

            let stream = TokioIo::new(stream);

            let token_verifier = LocalTokenVerifier::new(&secret);
            let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| {
                tower_service
                    .clone()
                    .with_state(ConnState {
                        agent_id: agent_id.clone(),
                        secret: secret.clone(),
                        token_verifier: token_verifier.clone(),
                    })
                    .call(request)
            });

            let ret = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(stream, hyper_service)
                .await;

            if let Err(err) = ret {
                warn!("error serving connection from {}: {}", addr, err);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read, path::PathBuf};

    use super::*;
    use modama_common::tokens::TokenFetcher;
    use modama_common::{ca::Ca, tokens::TokenVerifier};

    use modama_common::test_helpers::TempDir;
    use reqwest::{Certificate, Client};

    struct TestTokenServer {
        ca: Ca,
        ca_cert: PathBuf,
        tmpdir: TempDir,
        token_server: TokenServer,
    }

    async fn run_test_token_server() -> TestTokenServer {
        // create a temporary dir for certs (use local TempDir helper)
        let tmpdir = TempDir::new("test-token-server");

        // create CA and serialize
        let ca = Ca::new(Some(tmpdir.to_owned()), "hunter42").expect("create ca");
        ca.serialize_ca_to_files().expect("serialize ca");

        // create server cert
        let server_names = vec!["localhost".to_string()];
        let server_kc = ca.server_cert(&server_names).expect("server cert");
        ca.save_server_cert(&server_names, &server_kc)
            .expect("save server cert");

        // paths to server files
        let ca_cert = tmpdir.join("ca").join("pub").join("ca.cert.pem");
        let server_chain = tmpdir
            .join("servers")
            .join("localhost")
            .join("localhost.chain.pem");
        let server_priv = tmpdir
            .join("servers")
            .join("localhost")
            .join("localhost.priv.pem");

        let token_server = TokenServer::start("127.0.0.1:0", &ca_cert, &server_chain, &server_priv)
            .await
            .unwrap();

        TestTokenServer {
            ca,
            ca_cert,
            tmpdir,
            token_server,
        }
    }

    #[tokio::test]
    async fn test_token_server_sign_with_client_cert() {
        let test_server = run_test_token_server().await;
        // create client cert
        let agent_name = "test_agent";
        let resource_name = "test_resource";
        let client_kc = test_server
            .ca
            .client_cert(agent_name, resource_name)
            .expect("client cert");
        test_server
            .ca
            .save_client_cert(agent_name, resource_name, &client_kc)
            .expect("save client cert");

        // Use TokenFetcher to perform an authenticated request and TokenVerifier
        // to ensure the token can be verified by the server's verify endpoint.
        let client_key_path = test_server
            .tmpdir
            .join("clients")
            .join(resource_name)
            .join(format!("{}.priv.pem", agent_name));
        let client_chain_path = test_server
            .tmpdir
            .join("clients")
            .join(resource_name)
            .join(format!("{}.chain.pem", agent_name));

        let sign_url = format!(
            "https://localhost:{}/sign/",
            test_server.token_server.port()
        );
        let verify_url = format!(
            "https://localhost:{}/verify/",
            test_server.token_server.port()
        );

        let fetcher = TokenFetcher::from_files(
            sign_url.as_str(),
            &client_key_path,
            &client_chain_path,
            &test_server.ca_cert,
        )
        .expect("make token fetcher");

        // perform the request and get jwt
        let sign_response = fetcher.fetch_token().await.expect("fetch token");

        let verifier =
            TokenVerifier::new(verify_url.as_str(), &test_server.ca_cert).expect("make verifier");
        let resp = verifier.verify(&sign_response.token).await.expect("verify");
        assert_eq!(
            resp.get_agent_id().unwrap(),
            AgentId::from_uri("x-modama-agent://test_resource#test_agent").unwrap()
        );
        match resp {
            modama_common::tokens::TokenVerifyResponse::VerifiedAgent { agent_id } => {
                assert!(agent_id.contains(resource_name));
            }
            modama_common::tokens::TokenVerifyResponse::Invalid => panic!("token invalid"),
        }

        test_server.token_server.handle.abort();
    }

    /// broken token fetcher without client certificate - requests made using
    /// this guy should fail.
    pub struct TokenFetcherWithoutCertificate {
        client: Client,
        sign_service_url: String,
    }

    impl TokenFetcherWithoutCertificate {
        pub fn new(sign_service_url: &str, ca_cert_file: &Path) -> Self {
            let mut ca_certs = File::open(ca_cert_file).unwrap();
            let mut ca_pem: Vec<u8> = Vec::new();
            ca_certs.read_to_end(&mut ca_pem).unwrap();
            let root_cert = Certificate::from_pem(&ca_pem).unwrap();

            let client = modama_common::pinned_ca_client_builder(root_cert)
                .build()
                .unwrap();

            Self {
                client,
                sign_service_url: sign_service_url.to_owned(),
            }
        }

        pub async fn fetch_token(&self) -> reqwest::Response {
            self.client
                .put(&self.sign_service_url)
                .send()
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn test_without_client_cert() {
        let test_server = run_test_token_server().await;
        let sign_url = format!(
            "https://localhost:{}/sign/",
            test_server.token_server.port()
        );
        let fetcher = TokenFetcherWithoutCertificate::new(sign_url.as_str(), &test_server.ca_cert);
        let resp = fetcher.fetch_token().await;
        assert_eq!(resp.status(), 403);
        test_server.token_server.handle.abort();
    }
}
