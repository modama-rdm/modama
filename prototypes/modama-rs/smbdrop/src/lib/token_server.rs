use std::{
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Request, State},
    routing::put,
    Json, Router,
};
use hyper::{body::Incoming, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use log::{error, warn};
use modama_common::{agent::AgentId, tokens::TokenVerifyResponse};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower_service::Service;

use crate::{
    cert_helpers::{agent_id_from_stream, get_private_key, load_tls_config},
    events::ServerStartError,
};

#[derive(Clone)]
struct TokenServerState {}

#[derive(Clone)]
struct ConnState {
    pub agent_id: Option<AgentId>,
    pub secret: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: u64,
}

const EXPIRY_DURATION: u64 = 3600;

async fn sign_jwt(State(state): State<ConnState>) -> Result<String, StatusCode> {
    let agent_id = if let Some(agent_id) = &state.agent_id {
        agent_id.clone()
    } else {
        return Err(StatusCode::FORBIDDEN);
    };
    eprintln!("agent id {:?}", &agent_id);

    let sub = agent_id.san_encoded.as_str();
    let claims = Claims {
        sub: sub.to_owned(),
        exp: (SystemTime::now() + Duration::from_secs(EXPIRY_DURATION))
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&state.secret),
    )
    .unwrap())
}

async fn verify_jwt(
    State(state): State<ConnState>,
    request: String,
) -> Result<Json<TokenVerifyResponse>, StatusCode> {
    let mut validation = Validation::default();
    validation.validate_exp = true;  // in case the default _ever_ changes...

    Ok(Json(
        match decode::<Claims>(
            &request,
            &DecodingKey::from_secret(&state.secret),
            &validation,
        ) {
            Ok(decode_result) => TokenVerifyResponse::VerifiedAgent {
                agent_id: decode_result.claims.sub.clone(),
            },
            Err(e) => {
                warn!("error while validating jwt: {e} stuff");
                TokenVerifyResponse::Invalid
            },
        },
    ))
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
    let pk = get_private_key(privkey)?;
    let secret = Vec::from(pk.secret_der());

    let listener = TcpListener::bind(&listen_addr).await?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let app = Router::<ConnState>::new()
        .route("/sign/", put(sign_jwt))
        .route("/verify/", put(verify_jwt));

    loop {
        let tower_service = app.clone();
        let tls_acceptor = acceptor.clone();
        let secret = secret.clone();

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
                        secret: secret.clone(),
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
