use std::sync::Arc;

use axum::{extract::{Path, State}, http::{header, StatusCode}, response::IntoResponse, routing::{get, post}, Json, Router};
use axum_extra::{headers::{authorization::Basic, Authorization}, typed_header::TypedHeaderRejection, TypedHeader};
use axum_macros::debug_handler;
use log::{error, warn};
use minijinja::{context, Environment};
use modama_common::{messages::CurrentForegroundShare, mgmt_client::{MgmtClient, MgmtError}, tokens::TokenFetcher};
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

#[debug_handler]
async fn serve_index_page(
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> impl IntoResponse {
    let mut result = String::new();

    let auth = match auth {
        Ok(TypedHeader(auth)) => auth,
        Err(e) => {
            error!("error extracting authorization header: {e}");
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
    };

    let state = agent_state.state.lock().await;

    let matching_agents = state
        .connected_agents
        .iter()
        .filter(|a| a.resource == state.config.resource_name);

    result += "Connected agents at this resource: ";
    for a in matching_agents {
        result += &(a.as_uri().to_string() + " ");
    }
    result += "\n";
    result += &format!(
        "This agent: {} at {}",
        &state.config.agent_name, &state.config.resource_name
    );

    result += "\n";

    let c = MgmtClient::new("http://localhost:5001", auth.username(), auth.password()).unwrap();

    let projects = match c.list_projects().await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
        Err(e) => {
            error!("error listing projects: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
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

    result.into_response()
}

async fn start_ingest_step1(
    Path(project_name): Path<String>,
    State(_agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> impl IntoResponse {
    let auth = match auth {
        Ok(TypedHeader(auth)) => auth,
        Err(e) => {
            error!("error extracting authorization header: {e}");
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
    };

    let c = MgmtClient::new("http://localhost:5001", auth.username(), auth.password()).unwrap();

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };

    let mut resp = String::new();

    for d in &details.datasets {
        resp += &d.name;
        resp += "\n";
    }

    resp.into_response()
}

async fn start_ingest_step2(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> impl IntoResponse {
    let auth = match auth {
        Ok(TypedHeader(auth)) => auth,
        Err(e) => {
            error!("error extracting authorization header: {e}");
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
    };

    let c = MgmtClient::new("http://localhost:5001", auth.username(), auth.password()).unwrap();

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };

    let mut resp = String::new();

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return (StatusCode::NOT_FOUND).into_response();
    }

    resp += &format!(
        "Starting ingest for resource {} and session {}\n",
        agent_state.state.lock().await.config.resource_name,
        session_name,
    );

    let tpl = agent_state
        .jinja_env
        .get_template("start_index_step3.html")
        .unwrap();
    let res_name = &agent_state.state.lock().await.config.resource_name;
    (
        [(header::CONTENT_TYPE, "text/html")],
        tpl.render(context! { resource => res_name, session => session_name })
            .unwrap(),
    )
        .into_response()
}

async fn start_ingest_step3(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> impl IntoResponse {
    let auth = match auth {
        Ok(TypedHeader(auth)) => auth,
        Err(e) => {
            error!("error extracting authorization header: {e}");
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
    };

    let c = MgmtClient::new("http://localhost:5001", auth.username(), auth.password()).unwrap();

    let details = match c.project_details(&project_name).await {
        Ok(p) => p,
        Err(MgmtError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response();
        }
        Err(e) => {
            error!("error getting project details for {project_name}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
        }
    };

    let mut resp = String::new();

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return (StatusCode::NOT_FOUND).into_response();
    }

    let agent_token = agent_state.token_fetcher.fetch_token().await.unwrap();

    c.create_share(
        &project_name,
        &session_name,
        &agent_state.state.lock().await.config.resource_name,
        &agent_token,
    )
    .await
    .unwrap();

    resp += "share created!";

    resp.into_response()
}

pub async fn serve_local_web_api(
    state: Arc<Mutex<AgentState>>,
    listen_addr: &str,
    token_fetcher: TokenFetcher,
) {
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
    let listener = tokio::net::TcpListener::bind(listen_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}