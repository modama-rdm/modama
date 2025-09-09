use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    typed_header::TypedHeaderRejection,
    TypedHeader,
};
use axum_macros::debug_handler;
use log::{error, warn};
use minijinja::{context, Environment};
use modama_common::{
    messages::CurrentForegroundShare,
    mgmt_client::{MgmtClient, MgmtError},
    tokens::{TokenError, TokenFetcher},
};
use tokio::sync::Mutex;

use crate::agent_state::AgentState;

fn create_env() -> Environment<'static> {
    let mut env = Environment::new();

    env.set_loader(|name| match name {
        "start_index_step3.html" => Ok(Some(
            "<html><form action='.' method='post'><input type='submit'></form></html>".to_owned(),
        )),
        _ => Ok(None),
    });

    env
}

/// Wrapper around the AgentState for the local API server
#[derive(Clone)]
struct ApiState {
    pub state: Arc<Mutex<AgentState>>,
    pub token_fetcher: Arc<TokenFetcher>,
    pub jinja_env: Environment<'static>,
}

impl ApiState {
    fn new(state: &Arc<Mutex<AgentState>>, token_fetcher: &Arc<TokenFetcher>) -> Self {
        Self {
            state: Arc::clone(state),
            token_fetcher: Arc::clone(token_fetcher),
            jinja_env: create_env(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LocalAPIError {
    #[error("error in backend API: {0}")]
    BackendAPI(#[from] MgmtError),

    #[error("token error: {0}")]
    Token(#[from] TokenError),

    #[error("authentication failed")]
    Auth,

    #[error("template error: {0}")]
    Template(#[from] minijinja::Error),

    #[error("bind error: {0}")]
    Listen(#[from] std::io::Error),
}

impl IntoResponse for LocalAPIError {
    fn into_response(self) -> axum::response::Response {
        match self {
            LocalAPIError::BackendAPI(MgmtError::Unauthorized) | LocalAPIError::Auth => (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response(),
            LocalAPIError::BackendAPI(e) => {
                error!("backend API error: {e}");
                // FIXME: json/text depending on accept?
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "internal server error",
                )
                    .into_response()
            }
            LocalAPIError::Token(token_error) => {
                error!("token error: {token_error}");
                // content-type? status code? could be internal server error in some cases, ...?
                (
                    StatusCode::BAD_REQUEST,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "bad request",
                )
                    .into_response()
            }
            LocalAPIError::Template(error) => {
                error!("template error: {error}");
                // FIXME: json/text depending on accept?
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "internal server error",
                )
                    .into_response()
            }
            LocalAPIError::Listen(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain")],
                "internal server error",
            )
                .into_response(),
        }
    }
}

// #[debug_handler]
async fn handler_shares_foreground(
    State(api_state): State<ApiState>,
) -> Json<CurrentForegroundShare> {
    Json(
        match &api_state.state.lock().await.current_shares.foreground_share {
            None => CurrentForegroundShare::None,
            Some(s) => match s.mount_path.to_str() {
                None => {
                    let p = s.mount_path.to_string_lossy();
                    warn!("Mount path {p} contains invalid unicode, cannot get foreground share!");
                    CurrentForegroundShare::None
                }
                Some(mount_path) => CurrentForegroundShare::Share {
                    dest_path: s.share.dest_path.to_owned(),
                    mount_path: mount_path.to_owned(),
                },
            },
        },
    )
}

async fn get_auth(
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<Authorization<Basic>, LocalAPIError> {
    match auth {
        Ok(TypedHeader(auth)) => Ok(auth),
        Err(e) => {
            error!("error extracting authorization header: {e}");
            Err(LocalAPIError::Auth)
        }
    }
}

#[debug_handler]
async fn serve_index_page(
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let mut result = String::new();
    let auth = get_auth(auth).await?;

    let state = agent_state.state.lock().await;

    let matching_agents = state
        .connected_agents
        .iter()
        .filter(|a| a.resource == state.config.resource_name);

    result += "Connected agents at this resource: ";
    for a in matching_agents {
        result += &(a.san_encoded.to_owned() + " ");
    }
    result += "\n";
    result += &format!(
        "This agent: {} at {}",
        &state.config.agent_name, &state.config.resource_name
    );

    result += "\n";

    let c = MgmtClient::new(&state.config.mgmt_api_url, auth.username(), auth.password())?;

    let projects = match c.list_projects().await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response());
        }
        Err(e) => {
            error!("error listing projects: {e}");
            return Ok((StatusCode::INTERNAL_SERVER_ERROR).into_response());
        }
    };

    result += "Projects:\n";

    for p in projects
        .projects
        .iter()
        .filter(|p| p.members.contains(&auth.username().to_owned()) || p.owner == auth.username())
    {
        result += &p.name;
        result += "\n";
    }

    Ok(result.into_response())
}

async fn start_ingest_step1(
    Path(project_name): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let auth = get_auth(auth).await?;

    let c = {
        let state = agent_state.state.lock().await;
        MgmtClient::new(&state.config.mgmt_api_url, auth.username(), auth.password())?
    };

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response());
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return Ok((StatusCode::INTERNAL_SERVER_ERROR).into_response());
        }
    };

    let mut resp = String::new();

    for d in &details.datasets {
        resp += &d.name;
        resp += "\n";
    }

    Ok(resp.into_response())
}

async fn start_ingest_step2(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let auth = get_auth(auth).await?;

    let c = MgmtClient::new(
        &agent_state.state.lock().await.config.mgmt_api_url,
        auth.username(),
        auth.password(),
    )?;

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response());
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return Ok((StatusCode::INTERNAL_SERVER_ERROR).into_response());
        }
    };

    let mut resp = String::new();

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return Ok((StatusCode::NOT_FOUND).into_response());
    }

    resp += &format!(
        "Starting ingest for resource {} and session {}\n",
        agent_state.state.lock().await.config.resource_name,
        session_name,
    );

    let tpl = agent_state
        .jinja_env
        .get_template("start_index_step3.html")?;
    let res_name = &agent_state.state.lock().await.config.resource_name;
    Ok((
        [(header::CONTENT_TYPE, "text/html")],
        tpl.render(context! { resource => res_name, session => session_name })?,
    )
        .into_response())
}

async fn start_ingest_step3(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let auth = get_auth(auth).await?;

    let c = MgmtClient::new(
        &agent_state.state.lock().await.config.mgmt_api_url,
        auth.username(),
        auth.password(),
    )?;

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response());
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return Ok((StatusCode::INTERNAL_SERVER_ERROR).into_response());
        }
    };

    let mut resp = String::new();

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return Ok((StatusCode::NOT_FOUND).into_response());
    }

    let agent_token = agent_state.token_fetcher.fetch_token().await?;

    c.create_share(
        &project_name,
        &session_name,
        &agent_state.state.lock().await.config.resource_name,
        &agent_token,
    )
    .await?;

    resp += "share created!";

    Ok(resp.into_response())
}

pub async fn serve_local_web_api(
    state: Arc<Mutex<AgentState>>,
    listen_addr: &str,
    token_fetcher: TokenFetcher,
) -> Result<(), LocalAPIError> {
    let app = Router::new()
        .route("/shares/foreground/", get(handler_shares_foreground))
        .route("/", get(serve_index_page))
        .route("/start-ingest/:project_name/", get(start_ingest_step1))
        .route(
            "/start-ingest/:project_name/:session_name/",
            get(start_ingest_step2),
        )
        .route(
            "/start-ingest/:project_name/:session_name/",
            post(start_ingest_step3),
        )
        .with_state(ApiState::new(&state, &Arc::new(token_fetcher)));
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;

    #[allow(clippy::unwrap_used)] // infallible
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
