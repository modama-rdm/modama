use std::{path::Path, sync::Arc, collections::BTreeMap};

use axum::{
    extract::{Request, State},
    routing::{get, put},
    Json, Router,
};
use hmac::{Hmac, Mac};
use hyper::{body::Incoming, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use jwt::{SignWithKey, VerifyWithKey};
use log::{error, warn};
use modama_common::agent::AgentId;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower_service::Service;

use crate::{
    cert_helpers::{agent_id_from_stream, get_server_key, load_tls_config},
    events::ServerStartError,
};

#[derive(Clone)]
struct TokenServerState {}

#[derive(Clone, Debug)]
struct ConnState {
    pub agent_id: Option<AgentId>,
    pub key: Hmac<Sha256>,
}

async fn sign_jwt(State(state): State<ConnState>) -> Result<String, StatusCode> {
    let agent_id = if let Some(agent_id) = &state.agent_id {
        agent_id.clone()
    } else {
        return Err(StatusCode::FORBIDDEN);
    };
    eprintln!("agent id {:?}", &agent_id);

    let mut claims = BTreeMap::new();
    claims.insert("sub", agent_id.san_encoded.as_str());
    // FIXME: expiration claim!
    let token_str = claims.sign_with_key(&state.key).unwrap();
    Ok(token_str)
}

#[derive(Debug, Serialize, Deserialize)]
enum VerifyResponse {
    Invalid,
    VerifiedAgent {
        agent_id: String,
    }
}

async fn verify_jwt(
    State(state): State<ConnState>,
    request: String,
) -> Result<Json<VerifyResponse>, StatusCode> {
    let claims: BTreeMap<String, String> = request.verify_with_key(&state.key).unwrap();
    // FIXME: check expiration claim!

    Ok(Json(VerifyResponse::VerifiedAgent { agent_id: claims["sub"].clone() }))
}

/// Start a HTTP server on the given `listen_addr` with optional client
/// certificate verification. If a valid certificate for an agent is
/// presented, a JWT (JSON Web Token) for the agents' resource can be issued.
///
/// Without client certificate, the verification endpoint can be used
/// to check if a given JWT is valid.
pub async fn start_token_server(
    listen_addr: &str,
    ca_cert: &Path,
    chain: &Path,
    privkey: &Path,
) -> Result<(), ServerStartError> {
    let config = load_tls_config(ca_cert, chain, privkey, false)?;
    let sign_key: Hmac<Sha256> = Hmac::new_from_slice(get_server_key(privkey)?.secret_der()).unwrap();

    let listener = TcpListener::bind(&listen_addr).await?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let app = Router::<ConnState>::new()
        .route("/sign/", put(sign_jwt))
        .route("/verify/", put(verify_jwt));

    loop {
        let tower_service = app.clone();
        let tls_acceptor = acceptor.clone();
        let sign_key = sign_key.clone();

        let (conn, addr) = listener.accept().await.unwrap();

        tokio::spawn(async move {
            let Ok(stream) = tls_acceptor.accept(conn).await else {
                error!("error during handshake from {addr}");
                return;
            };

            let agent_id = agent_id_from_stream(&stream);

            let stream = TokioIo::new(stream);

            let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| {
                tower_service
                    .clone()
                    .with_state(ConnState {
                        agent_id: agent_id.clone(),
                        key: sign_key.clone(),
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
