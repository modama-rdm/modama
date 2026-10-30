//! Local minimal server-rendered ingest control UI.
//! Targets especially older browsers (Internet Explorer, ancient Firefox)
use axum::{
    Router,
    extract::{ConnectInfo, Form, Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Basic},
    typed_header::TypedHeaderRejection,
};
use axum_macros::debug_handler;
use log::error;
use minijinja::{AutoEscape, Environment, Value, context};
use modama_common::{
    local_api_messages::APICurrentForegroundShare,
    messages::Share,
    mgmt_client::{MgmtClient, MgmtError},
    tokens::{TokenError, TokenProvider},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::{error::Error, sync::Arc, time::Duration};
use time::{Date, OffsetDateTime};
use tokio::sync::Mutex;
use tower_http::csrf::CsrfLayer;

use crate::{
    agent_state::AgentState,
    caller_scope::{ConnInfo, HelperScope, PlatformScopeResolver, ResolveScope, ScopeError},
    config::{ConfigError, MTLSIdentity},
    events_api::add_internal_api,
    http_api::{add_api_routes, project_and_session},
};

fn create_env() -> Environment<'static> {
    let mut env = Environment::new();

    env.set_loader(|name| match name {
        "base.html" => Ok(Some(
            include_str!("templates/local_http/base.html").to_owned(),
        )),
        "index.html" => Ok(Some(
            include_str!("templates/local_http/index.html").to_owned(),
        )),
        "session_list.html" => Ok(Some(
            include_str!("templates/local_http/session_list.html").to_owned(),
        )),
        "start_confirm.html" => Ok(Some(
            include_str!("templates/local_http/start_confirm.html").to_owned(),
        )),
        "close_confirm.html" => Ok(Some(
            include_str!("templates/local_http/close_confirm.html").to_owned(),
        )),
        "message.html" => Ok(Some(
            include_str!("templates/local_http/message.html").to_owned(),
        )),
        _ => Ok(None),
    });
    env.set_auto_escape_callback(|_| AutoEscape::Html);

    env
}

#[derive(Serialize)]
struct LinkItem {
    name: String,
    href: Option<SafeLocalUrl>,
}

#[derive(Deserialize)]
struct CreateSessionForm {
    session_name: String,
}

#[derive(Serialize)]
struct ForegroundView {
    kind: &'static str,
    project: Option<String>,
    session: Option<String>,
    message: Option<String>,
    close_href: Option<SafeLocalUrl>,
    refresh_status: bool,
}

impl ForegroundView {
    fn new(foreground: &APICurrentForegroundShare) -> Self {
        match foreground {
            APICurrentForegroundShare::None => Self {
                kind: "none",
                project: None,
                session: None,
                message: None,
                close_href: None,
                refresh_status: false,
            },
            APICurrentForegroundShare::Unavailable {
                share_id,
                project,
                session,
                message,
                ..
            } => Self {
                kind: "unavailable",
                project: Some(project.clone()),
                session: Some(session.clone()),
                message: Some(message.clone()),
                close_href: Some(SafeLocalUrl::close_ingest(share_id)),
                refresh_status: true,
            },
            APICurrentForegroundShare::Mounted {
                share_id,
                project,
                session,
                ..
            } => Self {
                kind: "open",
                project: Some(project.clone()),
                session: Some(session.clone()),
                message: None,
                close_href: Some(SafeLocalUrl::close_ingest(share_id)),
                refresh_status: false,
            },
        }
    }
}

/// A same-origin path that can be inserted into HTML without further escaping.
///
/// This is deliberately not a general-purpose safe HTML string. Values can only
/// be built by the route-specific constructors below.
struct SafeLocalUrl(String);

impl SafeLocalUrl {
    fn start_project(project_name: &str) -> Self {
        Self::from_encoded_local_path(format!(
            "/start-ingest/{}/",
            encode_path_segment(project_name)
        ))
    }

    fn start_session(project_name: &str, session_name: &str) -> Self {
        Self::from_encoded_local_path(format!(
            "/start-ingest/{}/{}/",
            encode_path_segment(project_name),
            encode_path_segment(session_name)
        ))
    }

    fn close_ingest(share_id: &str) -> Self {
        Self::from_encoded_local_path(format!("/close-ingest/{}/", encode_path_segment(share_id)))
    }

    fn from_encoded_local_path(path: String) -> Self {
        // HTML-safety: the route constructors contribute only fixed ASCII
        // path syntax, and every dynamic segment is encoded with
        // `NON_ALPHANUMERIC`. Therefore the result can contain only ASCII
        // alphanumerics, '/', '-', and percent escapes. In particular it cannot
        // contain quotes, angle brackets, ampersands, whitespace, controls, or
        // ':'; it cannot escape a quoted attribute or name another origin.
        assert!(path.starts_with('/') && !path.starts_with("//"));
        assert!(
            path.bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'%') })
        );
        let bytes = path.as_bytes();
        assert!(bytes.iter().enumerate().all(|(index, byte)| {
            *byte != b'%'
                || (bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
                    && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit))
        }));
        Self(path)
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for SafeLocalUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // The constructor invariant above is the proof required before applying
        // MiniJinja's safe-string marker. Keeping the conversion here ensures
        // templates cannot opt an ordinary String out of autoescaping.
        Value::from_safe_string(self.0.clone()).serialize(serializer)
    }
}

fn encode_path_segment(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

fn render_html(
    env: &Environment<'static>,
    template_name: &str,
    status: StatusCode,
    context: Value,
) -> Result<Response, LocalAPIError> {
    let body = env.get_template(template_name)?.render(context)?;
    Ok((
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response())
}

fn message_page(
    env: &Environment<'static>,
    status: StatusCode,
    heading: &str,
    message: &str,
) -> Result<Response, LocalAPIError> {
    render_html(
        env,
        "message.html",
        status,
        context! { title => heading, heading, message },
    )
}

fn render_session_list(
    agent_state: &ApiState,
    status: StatusCode,
    project_name: &str,
    session_names: &[String],
    create_error: Option<&str>,
    new_session_name: &str,
    session_date: Date,
) -> Result<Response, LocalAPIError> {
    let sessions = session_names
        .iter()
        .map(|session_name| LinkItem {
            href: Some(SafeLocalUrl::start_session(project_name, session_name)),
            name: session_name.clone(),
        })
        .collect::<Vec<_>>();
    let create_action = SafeLocalUrl::start_project(project_name);
    let session_name_example = session_name_example(session_date);

    render_html(
        &agent_state.jinja_env,
        "session_list.html",
        status,
        context! {
            title => "Select or create a session",
            project => project_name,
            sessions,
            create_action,
            create_error,
            new_session_name,
            session_name_example,
        },
    )
}

fn today() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

fn session_name_prefix(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}-",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn session_name_example(date: Date) -> String {
    format!("{}alignment-run", session_name_prefix(date))
}

fn validate_session_name(input: &str) -> Result<String, &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Err("Please enter a session name.")
    } else if trimmed.contains('/') {
        Err("Session name cannot contain a slash (/).")
    } else {
        Ok(trimmed.to_owned())
    }
}

async fn local_snapshot(agent_state: &ApiState) -> (url::Url, String, Vec<String>) {
    let state = agent_state.state.lock().await;
    let (identity, resource_name) = match &state.config.identity {
        MTLSIdentity::Anonymous { hostname } => {
            (format!("Anonymous running on {hostname}"), hostname.clone())
        }
        MTLSIdentity::AgentId {
            agent_name,
            resource_name,
        } => (
            format!("{agent_name} at {resource_name}"),
            resource_name.clone(),
        ),
    };
    let agents = state
        .remote_info
        .as_ref()
        .map(|remote| {
            remote
                .connected_agents
                .iter()
                .filter(|agent| agent.resource == resource_name)
                .map(|agent| agent.san_encoded.clone())
                .collect()
        })
        .unwrap_or_default();
    (state.config.mgmt_api_url.clone(), identity, agents)
}

/// The desired foreground share; mounted or not, this is what the user can close, and what
/// prevents opening another ingest
async fn desired_foreground(agent_state: &ApiState) -> Option<Share> {
    agent_state
        .state
        .lock()
        .await
        .remote_info
        .as_ref()
        .and_then(|remote| remote.desired_shares.foreground_share.clone())
}

/// Wrapper around the AgentState for the local API server
#[derive(Clone)]
pub struct ApiState {
    pub state: Arc<Mutex<AgentState>>,
    pub token_provider: Arc<dyn TokenProvider + Send + Sync>,
    pub jinja_env: Environment<'static>,
    pub scope_resolver: Arc<dyn ResolveScope>,
}

impl ApiState {
    fn new(
        state: &Arc<Mutex<AgentState>>,
        token_provider: &Arc<dyn TokenProvider + Send + Sync>,
    ) -> Self {
        Self {
            state: Arc::clone(state),
            token_provider: Arc::clone(token_provider),
            jinja_env: create_env(),
            scope_resolver: Arc::new(PlatformScopeResolver),
        }
    }

    /// Determine the helper scope of the client on the other end of `conn`
    pub async fn resolve_scope(&self, conn: ConnInfo) -> Result<HelperScope, LocalAPIError> {
        let resolver = Arc::clone(&self.scope_resolver);
        // reading the socket table may take a moment on busy hosts:
        tokio::task::spawn_blocking(move || resolver.resolve(&conn))
            .await
            .map_err(|e| ScopeError::SocketTable(e.to_string()))?
            .map_err(LocalAPIError::from)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LocalAPIError {
    #[error("API route configuration: {0}")]
    RouteConfig(String),

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

    #[error("configuration: {0}")]
    Config(#[from] ConfigError),

    #[error("invalid API response: {0}")]
    InvalidResponse(#[from] crate::http_api::ShareDestinationError),

    #[error("caller scope: {0}")]
    Scope(#[from] ScopeError),
}

impl From<versioned::RouteConfigError> for LocalAPIError {
    fn from(value: versioned::RouteConfigError) -> Self {
        Self::RouteConfig(value.to_string())
    }
}

impl IntoResponse for LocalAPIError {
    fn into_response(self) -> axum::response::Response {
        match self {
            LocalAPIError::RouteConfig(error) => {
                error!("local API route configuration error: {error}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "internal server error: bad API route configuration",
                )
                    .into_response()
            }
            LocalAPIError::BackendAPI(MgmtError::Unauthorized) | LocalAPIError::Auth => (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=modama-agent")],
            )
                .into_response(),
            LocalAPIError::BackendAPI(MgmtError::NotFound) => {
                // FIXME: json/text depending on accept?
                (
                    StatusCode::NOT_FOUND,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "object not found",
                )
                    .into_response()
            }
            LocalAPIError::BackendAPI(e) => {
                error!("backend API error: {e}");
                if let MgmtError::RequestError(error) = e {
                    let mut error = &error as &(dyn Error + 'static);
                    while let Some(inner) = error.source() {
                        error!("  error details: {:?}", inner);
                        error = inner;
                    }
                }
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
            LocalAPIError::Listen(_error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain")],
                "internal server error",
            )
                .into_response(),
            LocalAPIError::Config(_config_error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain")],
                "internal server error: bad configuration",
            )
                .into_response(),
            LocalAPIError::InvalidResponse(error) => {
                error!("invalid API response: {error}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "internal server error: invalid response",
                )
                    .into_response()
            }
            LocalAPIError::Scope(error @ ScopeError::NotFound) => (
                StatusCode::FORBIDDEN,
                [(header::CONTENT_TYPE, "text/plain")],
                error.to_string(),
            )
                .into_response(),
            LocalAPIError::Scope(error @ ScopeError::SocketTable(_)) => {
                error!("{error}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "internal server error: could not determine the caller",
                )
                    .into_response()
            }
        }
    }
}

pub async fn get_auth(
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
    ConnectInfo(conn): ConnectInfo<ConnInfo>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<Response, LocalAPIError> {
    let auth = get_auth(auth).await?;
    let (mgmt_api_url, identity, connected_agents) = local_snapshot(&agent_state).await;
    let scope = agent_state.resolve_scope(conn).await?;
    let foreground = agent_state.state.lock().await.foreground_for(scope)?;
    let start_allowed = foreground == APICurrentForegroundShare::None;
    let c = MgmtClient::new(&mgmt_api_url, auth.username(), auth.password())?;
    let projects = c
        .list_projects()
        .await?
        .projects
        .into_iter()
        .filter(|project| {
            project
                .members
                .iter()
                .any(|member| member == auth.username())
                || project.owner == auth.username()
        })
        .map(|project| LinkItem {
            href: start_allowed.then(|| SafeLocalUrl::start_project(&project.name)),
            name: project.name,
        })
        .collect::<Vec<_>>();
    let foreground = ForegroundView::new(&foreground);

    render_html(
        &agent_state.jinja_env,
        "index.html",
        StatusCode::OK,
        context! {
            title => "modama agent",
            identity,
            connected_agents,
            foreground,
            projects,
            start_allowed,
        },
    )
}

async fn start_ingest_step1(
    Path(project_name): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<Response, LocalAPIError> {
    let auth = get_auth(auth).await?;

    if desired_foreground(&agent_state).await.is_some() {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest already active",
            "Close the current ingest before opening another one.",
        );
    }

    let c = {
        let state = agent_state.state.lock().await;
        MgmtClient::new(&state.config.mgmt_api_url, auth.username(), auth.password())?
    };

    let details = c.project_details(&project_name).await?;
    let session_names = details
        .datasets
        .into_iter()
        .map(|session| session.name)
        .collect::<Vec<_>>();

    let session_date = today();
    let default_session_name = session_name_prefix(session_date);
    render_session_list(
        &agent_state,
        StatusCode::OK,
        &project_name,
        &session_names,
        None,
        &default_session_name,
        session_date,
    )
}

async fn create_session(
    Path(project_name): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
    Form(form): Form<CreateSessionForm>,
) -> Result<Response, LocalAPIError> {
    let auth = get_auth(auth).await?;

    if desired_foreground(&agent_state).await.is_some() {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest already active",
            "Close the current ingest before creating another session.",
        );
    }

    let mgmt_api_url = agent_state.state.lock().await.config.mgmt_api_url.clone();
    let c = MgmtClient::new(&mgmt_api_url, auth.username(), auth.password())?;
    let details = c.project_details(&project_name).await?;
    let session_names = details
        .datasets
        .into_iter()
        .map(|session| session.name)
        .collect::<Vec<_>>();

    let session_name = match validate_session_name(&form.session_name) {
        Ok(session_name) => session_name,
        Err(message) => {
            return render_session_list(
                &agent_state,
                StatusCode::BAD_REQUEST,
                &project_name,
                &session_names,
                Some(message),
                &form.session_name,
                today(),
            );
        }
    };

    if desired_foreground(&agent_state).await.is_some() {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest already active",
            "Close the current ingest before creating another session.",
        );
    }

    if let Err(error) = c
        .create_dataset(&project_name, &session_name, auth.username())
        .await
    {
        if let MgmtError::Backend { message, .. } = error {
            let message = format!("Could not create the session: {message}");
            return render_session_list(
                &agent_state,
                StatusCode::BAD_REQUEST,
                &project_name,
                &session_names,
                Some(&message),
                &session_name,
                today(),
            );
        }
        return Err(error.into());
    }

    let confirmation_url = SafeLocalUrl::start_session(&project_name, &session_name);
    Ok(Redirect::to(confirmation_url.as_str()).into_response())
}

async fn start_ingest_step2(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<Response, LocalAPIError> {
    let auth = get_auth(auth).await?;

    if desired_foreground(&agent_state).await.is_some() {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest already active",
            "Close the current ingest before opening another one.",
        );
    }

    let (mgmt_api_url, _, _) = local_snapshot(&agent_state).await;
    let c = MgmtClient::new(&mgmt_api_url, auth.username(), auth.password())?;

    let details = c.project_details(&project_name).await?;

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::NOT_FOUND,
            "Session not found",
            "The selected session does not exist in this project.",
        );
    }

    let res_name = match &agent_state.state.lock().await.config.identity {
        MTLSIdentity::Anonymous { hostname } => hostname.clone(),
        MTLSIdentity::AgentId {
            agent_name: _,
            resource_name,
        } => resource_name.clone(),
    };
    let action = SafeLocalUrl::start_session(&project_name, &session_name);
    render_html(
        &agent_state.jinja_env,
        "start_confirm.html",
        StatusCode::OK,
        context! {
            title => "Open ingest",
            project => project_name,
            session => session_name,
            resource => res_name,
            action,
        },
    )
}

async fn start_ingest_step3(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<Response, LocalAPIError> {
    let auth = get_auth(auth).await?;

    if desired_foreground(&agent_state).await.is_some() {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest already active",
            "Close the current ingest before opening another one.",
        );
    }

    let mgmt_api_url = agent_state.state.lock().await.config.mgmt_api_url.clone();
    let c = MgmtClient::new(&mgmt_api_url, auth.username(), auth.password())?;

    let details = c.project_details(&project_name).await?;

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::NOT_FOUND,
            "Session not found",
            "The selected session does not exist in this project.",
        );
    }

    if desired_foreground(&agent_state).await.is_some() {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest already active",
            "Close the current ingest before opening another one.",
        );
    }

    let sign_response = agent_state.token_provider.fetch_token().await?;
    c.create_share(
        &project_name,
        &session_name,
        &sign_response.resource,
        &sign_response.token,
    )
    .await?;

    // there's a good chance the ingest share is available within 300ms; with this delay
    // the user doesn't have to refresh the overview page (hopefully in most cases!)
    tokio::time::sleep(Duration::from_millis(300)).await;

    Ok(Redirect::to("/").into_response())
}

async fn close_ingest_confirmation(
    Path(share_id): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<Response, LocalAPIError> {
    let _auth = get_auth(auth).await?;
    let Some(foreground) = desired_foreground(&agent_state)
        .await
        .filter(|share| share.share_id == share_id)
    else {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest cannot be closed",
            "This ingest is not the currently open foreground ingest.",
        );
    };
    let (project, session) = project_and_session(&foreground.dest_path)?;
    let action = SafeLocalUrl::close_ingest(&share_id);

    render_html(
        &agent_state.jinja_env,
        "close_confirm.html",
        StatusCode::OK,
        context! {
            title => "Close ingest",
            project,
            session,
            action,
        },
    )
}

async fn close_ingest(
    Path(share_id): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<Response, LocalAPIError> {
    let auth = get_auth(auth).await?;
    if desired_foreground(&agent_state)
        .await
        .is_none_or(|share| share.share_id != share_id)
    {
        return message_page(
            &agent_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest cannot be closed",
            "This ingest is not the currently open foreground ingest.",
        );
    }

    let mgmt_api_url = agent_state.state.lock().await.config.mgmt_api_url.clone();
    let c = MgmtClient::new(&mgmt_api_url, auth.username(), auth.password())?;
    c.delete_share(&share_id).await?;

    Ok(Redirect::to("/").into_response())
}

pub async fn serve_local_web_api(
    state: Arc<Mutex<AgentState>>,
    listen_addr: &str,
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
) -> Result<(), LocalAPIError> {
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    serve_local_web_api_on(listener, state, token_provider).await
}

pub async fn serve_local_web_api_on(
    listener: tokio::net::TcpListener,
    state: Arc<Mutex<AgentState>>,
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
) -> Result<(), LocalAPIError> {
    let app = make_app(ApiState::new(&state, &token_provider))?;

    #[allow(clippy::unwrap_used)] // infallible
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<ConnInfo>(),
    )
    .await
    .unwrap();

    Ok(())
}

fn make_app(state: ApiState) -> Result<Router, LocalAPIError> {
    let router = Router::new()
        .route("/", get(serve_index_page))
        .route(
            "/start-ingest/{project_name}/",
            get(start_ingest_step1).post(create_session),
        )
        .route(
            "/start-ingest/{project_name}/{session_name}/",
            get(start_ingest_step2),
        )
        .route(
            "/start-ingest/{project_name}/{session_name}/",
            post(start_ingest_step3),
        )
        .route("/close-ingest/{share_id}/", get(close_ingest_confirmation))
        .route("/close-ingest/{share_id}/", post(close_ingest));

    let router = add_internal_api(router)?;

    Ok(add_api_routes(router, state)?.layer(CsrfLayer::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_state::RemoteInfo;
    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use modama_common::internal_api_types::{MountedShare, SessionHelperState};
    use modama_common::messages::{self, Share as MessageShare};
    use modama_common::mgmt_client::{
        AdhocProject, DataInProject, ListProjectsResponse, ProjectDetailsResponse,
    };
    use modama_common::tokens::{BoxFutureToken, TokenSignResponse};
    use rustls::ClientConfig;
    use serde_json::Value;
    use std::path::PathBuf;
    use tokio_rustls::rustls::RootCertStore;
    use tower::ServiceExt;
    use url::Url;

    // Dummy token provider that returns a fixed token
    #[derive(Debug)]
    struct DummyTokenProvider;

    impl TokenProvider for DummyTokenProvider {
        fn fetch_token(self: Arc<Self>) -> BoxFutureToken {
            Box::pin(async move {
                Ok(TokenSignResponse {
                    token: "dummy-token".to_owned(),
                    resource: "resource".to_owned(),
                    agent_name: "agent_name".to_owned(),
                    agent_uri: "x-modama-agent://resource#agent".to_owned(),
                })
            })
        }
    }

    struct TestSetup {
        pub app: Router,
        pub shared_state: Arc<Mutex<AgentState>>,
    }

    impl TestSetup {
        pub async fn start() -> Self {
            let root = RootCertStore::empty();

            let cfg = crate::config::AgentConfig {
                events_api_url: Url::parse("http://localhost/").unwrap(),
                mgmt_api_url: Url::parse("http://localhost/").unwrap(),
                smb_hostname: "host".to_string(),
                smb_ip: tokio::sync::OnceCell::new(),
                smb_port: 445,
                smb_domain: "WINDOWS_FOR_WORKGROUPS".to_string(),
                identity: crate::config::MTLSIdentity::AgentId {
                    agent_name: "agent-name".to_string(),
                    resource_name: "res".to_string(),
                },
                listen_local_api: "localhost:3001".to_owned(),
                token_service_url: Url::parse("http://localhost/").unwrap(),
                ca_root_store: root.clone(),
                ca_root_cert: "".to_owned(),
                client_config: Arc::new(
                    ClientConfig::builder()
                        .with_root_certificates(root.clone())
                        .with_no_client_auth(),
                ),
                client_cert: None,
            };
            let shared_state = crate::agent_state::AgentState::new(&cfg).await;
            shared_state.lock().await.remote_info = Some(RemoteInfo::default());

            shared_state
                .lock()
                .await
                .remote_info
                .as_mut()
                .unwrap()
                .connected_agents
                .push(modama_common::agent::AgentId {
                    resource: "res".to_string(),
                    agent: "agent".to_string(),
                    san_encoded: "x-modama-agent://res#agent".to_string(),
                });

            let token_provider: Arc<dyn TokenProvider + Sync + Send> = Arc::new(DummyTokenProvider);
            let mut api_state = ApiState::new(&shared_state, &token_provider);
            api_state.scope_resolver = Arc::new(TestScope);
            let app = make_app(api_state)
                .unwrap()
                .layer(MockConnectInfo(ConnInfo {
                    local: Some(([127, 0, 0, 1], 3001).into()),
                    peer: ([127, 0, 0, 1], 50000).into(),
                }));

            Self { app, shared_state }
        }

        /// Make `share` the desired foreground share; if `mount_path` is given, the session
        /// helper of the test scope reports it as mounted there
        async fn set_foreground(&self, share: MessageShare, mount_path: Option<&str>) {
            let mut state = self.shared_state.lock().await;
            if let Some(mount_path) = mount_path {
                state.session_helpers.insert(
                    TEST_SCOPE,
                    SessionHelperState {
                        mounted_shares: vec![MountedShare {
                            share_name: share.share_name.clone(),
                            mount_path: Some(PathBuf::from(mount_path)),
                        }],
                        errors: vec![],
                    },
                );
            }
            state
                .remote_info
                .as_mut()
                .unwrap()
                .desired_shares
                .foreground_share = Some(share);
        }
    }

    const TEST_SCOPE: HelperScope = HelperScope::Uid(1001);

    /// Every request comes from the test scope
    struct TestScope;

    impl ResolveScope for TestScope {
        fn resolve(&self, _conn: &ConnInfo) -> Result<HelperScope, ScopeError> {
            Ok(TEST_SCOPE)
        }
    }

    fn foreground_share(share_id: &str) -> MessageShare {
        MessageShare {
            resource: Some("res".to_owned()),
            share_id: share_id.to_owned(),
            share_name: "share-name".to_owned(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            created_timestamp: 0,
            username: "share-user".to_owned(),
            password: "secret-password".to_owned(),
            uid: 1001,
            gid: 1001,
        }
    }

    async fn set_open_foreground(test_setup: &TestSetup, share_id: &str) {
        test_setup
            .set_foreground(foreground_share(share_id), Some("/mnt/ingest"))
            .await;
    }

    #[tokio::test]
    async fn csrf_layer_rejects_cross_site_unsafe_requests() {
        let test_setup = TestSetup::start().await;
        let request = Request::builder()
            .uri("/close-ingest/share-id/")
            .method("POST")
            .header(header::HOST, "localhost:3001")
            .header("Sec-Fetch-Site", "cross-site")
            .body(Body::empty())
            .unwrap();

        let response = test_setup.app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn csrf_layer_rejects_unsafe_requests_from_a_foreign_origin() {
        let test_setup = TestSetup::start().await;
        let request = Request::builder()
            .uri("/close-ingest/share-id/")
            .method("POST")
            .header(header::HOST, "localhost:3001")
            .header(header::ORIGIN, "https://attacker.example")
            .body(Body::empty())
            .unwrap();

        let response = test_setup.app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn csrf_layer_allows_same_origin_and_native_unsafe_requests() {
        for origin in [Some("http://localhost:3001"), None] {
            let test_setup = TestSetup::start().await;
            let mut request = Request::builder()
                .uri("/close-ingest/share-id/")
                .method("POST")
                .header(header::HOST, "localhost:3001");
            if let Some(origin) = origin {
                request = request.header(header::ORIGIN, origin);
            }

            let response = test_setup
                .app
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn csrf_layer_allows_cross_site_safe_requests() {
        let test_setup = TestSetup::start().await;
        let request = Request::builder()
            .uri("/")
            .method("GET")
            .header(header::HOST, "localhost:3001")
            .header("Sec-Fetch-Site", "cross-site")
            .body(Body::empty())
            .unwrap();

        let response = test_setup.app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    fn project_details(session_names: &[&str]) -> ProjectDetailsResponse {
        ProjectDetailsResponse {
            errno: 0,
            project: AdhocProject {
                relpath: "the-project".to_owned(),
                name: "the-project".to_owned(),
                owner: "testuser".to_owned(),
                members: vec!["testuser".to_owned()],
                ctime: 0.0,
                mtime: 0.0,
                gid: 1001,
                usage_bytes: None,
            },
            datasets: session_names
                .iter()
                .map(|name| DataInProject {
                    name: (*name).to_owned(),
                    immutable: false,
                    ctime: 0.0,
                    mtime: 0.0,
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn test_shares_foreground_endpoint_returns_share() {
        let test_setup = TestSetup::start().await;

        let share = MessageShare {
            resource: None,
            share_id: "id".to_string(),
            share_name: "sname".to_string(),
            dest_path: "/adhoc/project/session/".to_string(),
            created_timestamp: 0,
            username: "u".to_string(),
            password: "p".to_string(),
            uid: 1000,
            gid: 1000,
        };
        test_setup.set_foreground(share, Some("/mnt/point")).await;
        let req = Request::builder()
            .uri("/api/shares/foreground/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let got: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(got["type"], "Mounted");
        assert_eq!(got["share_id"], "id");
        assert_eq!(got["project"], "project");
        assert_eq!(got["session"], "session");
        assert_eq!(got["dest_path"], "/adhoc/project/session/");
        assert_eq!(got["mount_path"], "/mnt/point");
    }

    #[tokio::test]
    async fn foreground_endpoint_without_helper_says_how_to_start_one() {
        let test_setup = TestSetup::start().await;
        test_setup
            .set_foreground(foreground_share("id"), None)
            .await;

        let req = Request::builder()
            .uri("/api/shares/foreground/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let got: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(got["type"], "Unavailable");
        assert_eq!(got["share_id"], "id");
        assert!(
            got["message"]
                .as_str()
                .unwrap()
                .contains("modama-agent session-helper")
        );
        assert!(!bytes.windows(15).any(|w| w == b"secret-password"));
    }

    #[tokio::test]
    async fn test_no_fg_share_by_default() {
        let test_setup = TestSetup::start().await;

        let req = Request::builder()
            .uri("/api/shares/foreground/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let got: Value = serde_json::from_slice(&bytes).unwrap();
        println!("{got}");
        assert_eq!(got["type"], "None");
    }

    #[tokio::test]
    async fn test_identity_is_unauthenticated() {
        let test_setup = TestSetup::start().await;
        let req = Request::builder()
            .uri("/api/identity/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let got: Value =
            serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(got["type"], "AgentId");
        assert_eq!(got["agent_name"], "agent-name");
        assert_eq!(got["resource_name"], "res");
    }

    #[tokio::test]
    async fn malformed_foreground_destination_is_an_api_error() {
        let test_setup = TestSetup::start().await;
        let share = MessageShare {
            resource: None,
            share_id: "id".to_owned(),
            share_name: "share".to_owned(),
            dest_path: "session-only".to_owned(),
            created_timestamp: 0,
            username: "user".to_owned(),
            password: "secret".to_owned(),
            uid: 1000,
            gid: 1000,
        };
        test_setup.set_foreground(share, Some("/mnt/share")).await;
        let req = Request::builder()
            .uri("/api/shares/foreground/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_version_header_is_required_for_api_routes() {
        let test_setup = TestSetup::start().await;

        let req = Request::builder()
            .uri("/api/health/")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let got: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(got["type"], "missing_version");
        assert_eq!(got["header"], "X-API-Version");
    }

    #[tokio::test]
    async fn test_invalid_or_unsupported_api_version_is_rejected() {
        for version in ["not-a-version", "0.0.0"] {
            let test_setup = TestSetup::start().await;
            let req = Request::builder()
                .uri("/api/health/")
                .method("GET")
                .header("X-API-Version", version)
                .body(Body::empty())
                .unwrap();

            let resp = test_setup.app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            let got: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(matches!(
                got["type"].as_str(),
                Some("invalid_version") | Some("unsupported_version")
            ));
        }
    }

    #[tokio::test]
    async fn test_openapi_endpoint_describes_latest_api() {
        let test_setup = TestSetup::start().await;

        let req = Request::builder()
            .uri("/openapi.json")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let got: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(got["info"]["title"], "modama-agent local API");
        assert_eq!(got["info"]["version"], "1.0.0");

        let paths = got["paths"].as_object().unwrap();
        assert!(paths.contains_key("/api/health/"));
        assert!(paths.contains_key("/api/identity/"));
        assert!(paths.contains_key("/api/login/"));
        assert!(paths.contains_key("/api/projects/"));
        assert!(paths.contains_key("/api/projects/{project_name}/sessions/"));
        assert!(
            paths
                .contains_key("/api/projects/{project_name}/sessions/{session_name}/start-ingest/")
        );
        assert!(paths.contains_key("/api/shares/foreground/"));
        assert!(paths.contains_key("/api/shares/{share_id}/"));
        assert!(got["components"]["schemas"]["APIListProjectsResponse"].is_object());
        assert!(got["components"]["securitySchemes"]["basic_auth"].is_object());
    }

    #[tokio::test]
    async fn test_index_page_shows_connected_agents_and_projects() {
        let test_setup = TestSetup::start().await;

        let list_projects_resp: ListProjectsResponse = ListProjectsResponse {
            errno: 0,
            projects: vec![AdhocProject {
                relpath: "the-project".to_owned(),
                name: "the-project".to_owned(),
                owner: "testuser".to_owned(),
                members: vec!["testuser".to_owned()],
                ctime: 0.0,
                mtime: 0.0,
                gid: 1001,
                usage_bytes: Some(42),
            }],
        };

        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&list_projects_resp).unwrap())
            .create_async()
            .await;

        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        // Build request to index with Basic auth for user 'testuser'
        // Precomputed base64 for "testuser:pass" is dGVzdHVzZXI6cGFzcw==
        let req = Request::builder()
            .uri("/")
            .method("GET")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body_str.contains("x-modama-agent:&#x2f;&#x2f;res#agent"));
        assert!(body_str.contains("agent-name"));
        assert!(body_str.contains("the-project"));
        assert!(body_str.contains("Select a project to start an ingest"));
        assert!(body_str.contains("href=\"/start-ingest/the%2Dproject/\""));
        assert!(!body_str.contains("<script"));
        assert!(body_str.contains("<style type=\"text/css\">"));
        assert!(body_str.contains("<div id=\"content\">"));
        assert!(body_str.contains("background-color: #f1f5fd;"));
        assert!(body_str.contains("background-color: #ffffff;"));
        assert!(body_str.contains("color: #0f172a;"));
        assert!(body_str.contains("max-width: 50em;"));
    }

    #[test]
    fn html_values_are_escaped_and_route_segments_are_encoded() {
        let env = create_env();
        let rendered = env
            .get_template("message.html")
            .unwrap()
            .render(context! {
                title => "Unsafe <title>",
                heading => "Unsafe <heading>",
                message => "<script>alert('unsafe')</script>",
            })
            .unwrap();

        assert!(!rendered.contains("<script>"));
        assert!(rendered.contains("&lt;script&gt;"));
        assert_eq!(
            encode_path_segment("old/project & session"),
            "old%2Fproject%20%26%20session"
        );

        for template in [
            include_str!("templates/local_http/base.html"),
            include_str!("templates/local_http/index.html"),
            include_str!("templates/local_http/session_list.html"),
            include_str!("templates/local_http/start_confirm.html"),
            include_str!("templates/local_http/close_confirm.html"),
            include_str!("templates/local_http/message.html"),
        ] {
            assert!(!template.contains("|safe"));
        }

        let url = SafeLocalUrl::start_session(
            "project\"><script>alert(1)</script>",
            "session&external=//example.com",
        );
        let template_value = minijinja::Value::from_serialize(&url);
        assert!(template_value.is_safe());
        assert_eq!(template_value.as_str(), Some(url.as_str()));
        assert!(url.as_str().starts_with("/start-ingest/"));
        assert!(!url.as_str().contains(['\"', '<', '>', '&', ':']));
    }

    #[tokio::test]
    async fn index_offers_close_for_an_open_foreground_ingest() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "share-123").await;

        let projects = ListProjectsResponse {
            errno: 0,
            projects: vec![AdhocProject {
                relpath: "the-project".to_owned(),
                name: "the-project".to_owned(),
                owner: "testuser".to_owned(),
                members: vec![],
                ctime: 0.0,
                mtime: 0.0,
                gid: 1001,
                usage_bytes: None,
            }],
        };
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&projects).unwrap())
            .create_async()
            .await;
        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        let req = Request::builder()
            .uri("/")
            .method("GET")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();
        let resp = test_setup.app.oneshot(req).await.unwrap();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&body);

        assert!(body.contains("Status: open"));
        assert!(body.contains("href=\"/close-ingest/share%2D123/\""));
        assert!(!body.contains("href=\"/start-ingest/"));
        assert!(!body.contains("Reload this page"));
        assert!(!body.contains("secret-password"));
    }

    #[test]
    fn unavailable_foreground_shows_the_message_and_instructs_to_reload() {
        let unavailable = APICurrentForegroundShare::Unavailable {
            share_id: "share-123".to_owned(),
            project: "the-project".to_owned(),
            session: "test-session".to_owned(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            message: "Waiting for the session helper to mount the share.".to_owned(),
        };

        {
            let foreground = ForegroundView::new(&unavailable);
            let rendered = create_env()
                .get_template("index.html")
                .unwrap()
                .render(context! {
                    title => "modama agent",
                    identity => "agent at resource",
                    connected_agents => Vec::<String>::new(),
                    foreground,
                    projects => vec![LinkItem {
                        name: "project".to_owned(),
                        href: None,
                    }],
                    start_allowed => false,
                })
                .unwrap();

            assert!(rendered.contains("Status: unavailable."));
            assert!(rendered.contains("Waiting for the session helper to mount the share."));
            assert!(rendered.contains("<a href=\"/\">Reload this page</a>"));
            assert!(rendered.contains("to refresh the status."));
            // an unavailable ingest can still be closed, e.g. when no helper is running:
            assert!(rendered.contains("href=\"/close-ingest/share%2D123/\""));
            assert!(rendered.contains("Close the current ingest"));
            assert!(!rendered.contains("href=\"/start-ingest/"));
        }
    }

    #[tokio::test]
    async fn test_ingest_step1() {
        let test_setup = TestSetup::start().await;

        let proj = AdhocProject {
            relpath: "the-project".to_owned(),
            name: "the-project".to_owned(),
            owner: "testuser".to_owned(),
            members: vec!["testuser".to_owned()],
            ctime: 0.0,
            mtime: 0.0,
            gid: 1001,
            usage_bytes: Some(42),
        };

        let list_projects_resp: ListProjectsResponse = ListProjectsResponse {
            errno: 0,
            projects: vec![proj.clone()],
        };

        let project_details_resp: ProjectDetailsResponse = ProjectDetailsResponse {
            errno: 0,
            project: proj.clone(),
            datasets: vec![DataInProject {
                name: "test-session".to_owned(),
                immutable: false,
                ctime: 0.0,
                mtime: 0.0,
            }],
        };

        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&list_projects_resp).unwrap())
            .create_async()
            .await;

        server
            .mock("GET", "/api/project/the-project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&project_details_resp).unwrap())
            .create_async()
            .await;

        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        // Build request to index with Basic auth for user 'testuser'
        // Precomputed base64 for "testuser:pass" is dGVzdHVzZXI6cGFzcw==
        let req = Request::builder()
            .uri("/start-ingest/the-project/")
            .method("GET")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let date_before_request = today();
        let resp = test_setup.app.oneshot(req).await.unwrap();
        let date_after_request = today();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();

        assert!(body_str.contains("test-session"));
        assert!(body_str.contains("Select an existing session to start an ingest"));
        assert!(body_str.contains("href=\"/start-ingest/the%2Dproject/test%2Dsession/\""));
        assert!(body_str.contains("Create a new session"));
        assert!(body_str.contains("<form action=\"/start-ingest/the%2Dproject/\" method=\"post\""));
        assert!(body_str.contains("name=\"session_name\""));
        assert!(
            [date_before_request, date_after_request]
                .into_iter()
                .any(|date| {
                    body_str.contains(&format!("value=\"{}\"", session_name_prefix(date)))
                        && body_str.contains(&session_name_example(date))
                })
        );
    }

    #[test]
    fn validates_new_session_names() {
        assert_eq!(
            validate_session_name("  2026-08-11-run  "),
            Ok("2026-08-11-run".to_owned())
        );
        assert_eq!(
            validate_session_name("   "),
            Err("Please enter a session name.")
        );
        assert_eq!(
            validate_session_name("2026-08-11/run"),
            Err("Session name cannot contain a slash (/).")
        );
    }

    #[test]
    fn formats_session_name_defaults() {
        let date = Date::from_calendar_date(2026, time::Month::January, 3).unwrap();

        assert_eq!(session_name_prefix(date), "2026-01-03-");
        assert_eq!(session_name_example(date), "2026-01-03-alignment-run");
    }

    #[tokio::test]
    async fn create_session_redirects_to_start_confirmation() {
        let test_setup = TestSetup::start().await;
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/the-project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&project_details(&["existing"])).unwrap())
            .create_async()
            .await;
        let create = server
            .mock("POST", "/api/project/the-project/dataset/")
            .match_header("authorization", "Basic dGVzdHVzZXI6cGFzcw==")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "name": "new-session",
                "owner": "testuser",
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"errno":0,"message":null}"#)
            .create_async()
            .await;
        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        let req = Request::builder()
            .uri("/start-ingest/the-project/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("session_name=++new-session++"))
            .unwrap();
        let resp = test_setup.app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "/start-ingest/the%2Dproject/new%2Dsession/"
        );
        create.assert_async().await;
    }

    #[tokio::test]
    async fn invalid_new_session_name_redisplays_the_form_without_creating() {
        let test_setup = TestSetup::start().await;
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/the-project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&project_details(&["existing"])).unwrap())
            .create_async()
            .await;
        let create = server
            .mock("POST", "/api/project/the-project/dataset/")
            .expect(0)
            .create_async()
            .await;
        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        let req = Request::builder()
            .uri("/start-ingest/the-project/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("session_name=bad%2Fname"))
            .unwrap();
        let resp = test_setup.app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("Session name cannot contain a slash"));
        assert!(body.contains("value=\"bad&#x2f;name\""));
        assert!(body.contains("existing"));
        create.assert_async().await;
    }

    #[tokio::test]
    async fn session_creation_rejection_is_escaped_and_shown_inline() {
        let test_setup = TestSetup::start().await;
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/the-project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&project_details(&[])).unwrap())
            .create_async()
            .await;
        server
            .mock("POST", "/api/project/the-project/dataset/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"errno":17,"message":"session <exists>"}"#)
            .create_async()
            .await;
        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        let req = Request::builder()
            .uri("/start-ingest/the-project/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("session_name=new-session"))
            .unwrap();
        let resp = test_setup.app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("Could not create the session: session &lt;exists&gt;"));
        assert!(!body.contains("session <exists>"));
        assert!(body.contains("value=\"new-session\""));
    }

    #[tokio::test]
    async fn active_foreground_ingest_prevents_creating_a_session() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "current-share").await;

        let req = Request::builder()
            .uri("/start-ingest/the-project/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("session_name=new-session"))
            .unwrap();
        let resp = test_setup.app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_ingest_step2_invalid_session_name() {
        let test_setup = TestSetup::start().await;

        let proj = AdhocProject {
            relpath: "the-project".to_owned(),
            name: "the-project".to_owned(),
            owner: "testuser".to_owned(),
            members: vec!["testuser".to_owned()],
            ctime: 0.0,
            mtime: 0.0,
            gid: 1001,
            usage_bytes: Some(42),
        };

        let list_projects_resp: ListProjectsResponse = ListProjectsResponse {
            errno: 0,
            projects: vec![proj.clone()],
        };

        let project_details_resp: ProjectDetailsResponse = ProjectDetailsResponse {
            errno: 0,
            project: proj.clone(),
            datasets: vec![DataInProject {
                name: "test-session".to_owned(),
                immutable: false,
                ctime: 0.0,
                mtime: 0.0,
            }],
        };

        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&list_projects_resp).unwrap())
            .create_async()
            .await;

        server
            .mock("GET", "/api/project/the-project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&project_details_resp).unwrap())
            .create_async()
            .await;

        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        // Build request to index with Basic auth for user 'testuser'
        // Precomputed base64 for "testuser:pass" is dGVzdHVzZXI6cGFzcw==
        let req = Request::builder()
            .uri("/start-ingest/the-project/invalid-session/")
            .method("GET")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_ingest_step2_valid() {
        let test_setup = TestSetup::start().await;

        let proj = AdhocProject {
            relpath: "the-project".to_owned(),
            name: "the-project".to_owned(),
            owner: "testuser".to_owned(),
            members: vec!["testuser".to_owned()],
            ctime: 0.0,
            mtime: 0.0,
            gid: 1001,
            usage_bytes: Some(42),
        };

        let list_projects_resp: ListProjectsResponse = ListProjectsResponse {
            errno: 0,
            projects: vec![proj.clone()],
        };

        let project_details_resp: ProjectDetailsResponse = ProjectDetailsResponse {
            errno: 0,
            project: proj.clone(),
            datasets: vec![DataInProject {
                name: "test-session".to_owned(),
                immutable: false,
                ctime: 0.0,
                mtime: 0.0,
            }],
        };

        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&list_projects_resp).unwrap())
            .create_async()
            .await;

        server
            .mock("GET", "/api/project/the-project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&project_details_resp).unwrap())
            .create_async()
            .await;

        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        // Build request to index with Basic auth for user 'testuser'
        // Precomputed base64 for "testuser:pass" is dGVzdHVzZXI6cGFzcw==
        let req = Request::builder()
            .uri("/start-ingest/the-project/test-session/")
            .method("GET")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();

        assert!(body_str.contains(
            "<form action=\"/start-ingest/the%2Dproject/test%2Dsession/\" method=\"post\""
        ));
        assert!(body_str.contains("Project: the-project"));
        assert!(body_str.contains("Session: test-session"));
        assert!(body_str.contains("Resource: res"));
    }

    #[tokio::test]
    async fn test_ingest_step2_invalid_project_name() {
        let test_setup = TestSetup::start().await;

        let mut server = mockito::Server::new_async().await;
        // explicit 404 response for the invalid project name
        let mock = server
            .mock("GET", "/api/project/invalid-project/")
            .with_status(404)
            .with_header("content-type", "text/plain")
            .with_body("not found")
            .create_async()
            .await;

        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        // Build request to index with Basic auth for user 'testuser'
        // Precomputed base64 for "testuser:pass" is dGVzdHVzZXI6cGFzcw==
        let req = Request::builder()
            .uri("/start-ingest/invalid-project/test-session/")
            .method("GET")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_ingest_step3() {
        let test_setup = TestSetup::start().await;

        let proj = AdhocProject {
            relpath: "the-project".to_owned(),
            name: "the-project".to_owned(),
            owner: "testuser".to_owned(),
            members: vec!["testuser".to_owned()],
            ctime: 0.0,
            mtime: 0.0,
            gid: 1001,
            usage_bytes: Some(42),
        };

        let project_details_resp: ProjectDetailsResponse = ProjectDetailsResponse {
            errno: 0,
            project: proj.clone(),
            datasets: vec![DataInProject {
                name: "test-session".to_owned(),
                immutable: false,
                ctime: 0.0,
                mtime: 0.0,
            }],
        };

        let mut server = mockito::Server::new_async().await;
        let mock_details = server
            .mock("GET", "/api/project/the-project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&project_details_resp).unwrap())
            .create_async()
            .await;

        let share_response = messages::Share {
            resource: Some("res".to_owned()),
            share_id: "share-123".to_owned(),
            share_name: "share-123".to_owned(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            created_timestamp: 0,
            username: "share-user".to_owned(),
            password: "share-pass".to_owned(),
            uid: 1001,
            gid: 1001,
        };

        let mock_create_share = server
            .mock("POST", "/api/share/")
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&share_response).unwrap())
            .create_async()
            .await;

        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        // Build request to index with Basic auth for user 'testuser'
        // Precomputed base64 for "testuser:pass" is dGVzdHVzZXI6cGFzcw==
        let req = Request::builder()
            .uri("/start-ingest/the-project/test-session/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        mock_details.assert_async().await;
        mock_create_share.assert_async().await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/");
    }

    #[tokio::test]
    async fn test_ingest_step3_invalid_session_name() {
        let test_setup = TestSetup::start().await;

        let proj = AdhocProject {
            relpath: "the-project".to_owned(),
            name: "the-project".to_owned(),
            owner: "testuser".to_owned(),
            members: vec!["testuser".to_owned()],
            ctime: 0.0,
            mtime: 0.0,
            gid: 1001,
            usage_bytes: Some(42),
        };

        let project_details_resp: ProjectDetailsResponse = ProjectDetailsResponse {
            errno: 0,
            project: proj.clone(),
            datasets: vec![DataInProject {
                name: "test-session".to_owned(),
                immutable: false,
                ctime: 0.0,
                mtime: 0.0,
            }],
        };

        let mut server = mockito::Server::new_async().await;
        let mock_details = server
            .mock("GET", "/api/project/the-project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&project_details_resp).unwrap())
            .create_async()
            .await;

        let share_response = messages::Share {
            resource: Some("res".to_owned()),
            share_id: "share-123".to_owned(),
            share_name: "share-123".to_owned(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            created_timestamp: 0,
            username: "share-user".to_owned(),
            password: "share-pass".to_owned(),
            uid: 1001,
            gid: 1001,
        };

        let mock_create_share = server
            .mock("POST", "/api/share/")
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&share_response).unwrap())
            .expect_at_most(0)
            .create_async()
            .await;

        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        // Build request to index with Basic auth for user 'testuser'
        // Precomputed base64 for "testuser:pass" is dGVzdHVzZXI6cGFzcw==
        let req = Request::builder()
            .uri("/start-ingest/the-project/invalid-session/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        mock_details.assert_async().await;
        mock_create_share.assert_async().await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn active_foreground_ingest_prevents_starting_another() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "current-share").await;

        let req = Request::builder()
            .uri("/start-ingest/the-project/test-session/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();
        let resp = test_setup.app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("Close the current ingest"));
    }

    #[tokio::test]
    async fn close_ingest_requires_confirmation_and_redirects_after_deletion() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "share-123").await;

        let mut server = mockito::Server::new_async().await;
        let delete = server
            .mock("DELETE", "/api/share/share-123/")
            .match_header("authorization", "Basic dGVzdHVzZXI6cGFzcw==")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ok"}"#)
            .create_async()
            .await;
        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();

        let req = Request::builder()
            .uri("/close-ingest/share-123/")
            .method("GET")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();
        let resp = test_setup.app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("Project: the-project"));
        assert!(body.contains("Session: test-session"));
        assert!(body.contains("<form action=\"/close-ingest/share%2D123/\" method=\"post\""));
        assert!(!body.contains("secret-password"));

        let req = Request::builder()
            .uri("/close-ingest/share-123/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();
        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/");
        delete.assert_async().await;
    }

    #[tokio::test]
    async fn stale_close_link_does_not_delete_a_share() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "current-share").await;

        let req = Request::builder()
            .uri("/close-ingest/stale-share/")
            .method("POST")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();
        let resp = test_setup.app.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn local_api_lists_enriched_projects() {
        let test_setup = TestSetup::start().await;
        let projects = ListProjectsResponse {
            errno: 0,
            projects: vec![AdhocProject {
                relpath: "project".to_owned(),
                name: "Project One".to_owned(),
                owner: "owner".to_owned(),
                members: vec!["member".to_owned()],
                ctime: 10.0,
                mtime: 1_782_432_123.75,
                gid: 1000,
                usage_bytes: None,
            }],
        };
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/project/")
            .match_header("authorization", "Basic dGVzdHVzZXI6cGFzcw==")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&projects).unwrap())
            .create_async()
            .await;
        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();
        let req = Request::builder()
            .uri("/api/projects/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let got: Value =
            serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(got["projects"][0]["name"], "Project One");
        assert_eq!(got["projects"][0]["owner"], "owner");
        assert_eq!(got["projects"][0]["members"][0], "member");
        assert_eq!(got["projects"][0]["modified_at"], 1_782_432_123_i64);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn local_api_deletes_share_without_returning_management_body() {
        let test_setup = TestSetup::start().await;
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("DELETE", "/api/share/share-123/")
            .match_header("authorization", "Basic dGVzdHVzZXI6cGFzcw==")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ok"}"#)
            .create_async()
            .await;
        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();
        let req = Request::builder()
            .uri("/api/shares/share-123/")
            .method("DELETE")
            .header("X-API-Version", "1.0.0")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn local_api_start_response_contains_only_safe_share_metadata() {
        let test_setup = TestSetup::start().await;
        let project = AdhocProject {
            relpath: "project".to_owned(),
            name: "project".to_owned(),
            owner: "testuser".to_owned(),
            members: vec!["testuser".to_owned()],
            ctime: 0.0,
            mtime: 0.0,
            gid: 1000,
            usage_bytes: None,
        };
        let details = ProjectDetailsResponse {
            errno: 0,
            project,
            datasets: vec![DataInProject {
                name: "session".to_owned(),
                immutable: false,
                ctime: 0.0,
                mtime: 0.0,
            }],
        };
        let share = messages::Share {
            resource: Some("resource".to_owned()),
            share_id: "share-123".to_owned(),
            share_name: "share-name".to_owned(),
            dest_path: "/adhoc/project/session/".to_owned(),
            created_timestamp: 0,
            username: "temporary-user".to_owned(),
            password: "temporary-secret".to_owned(),
            uid: 1000,
            gid: 1000,
        };
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/project/project/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&details).unwrap())
            .create_async()
            .await;
        server
            .mock("POST", "/api/share/")
            .match_header("x-agent-token", "dummy-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&share).unwrap())
            .create_async()
            .await;
        test_setup.shared_state.lock().await.config.mgmt_api_url =
            Url::parse(&server.url()).unwrap();
        let req = Request::builder()
            .uri("/api/projects/project/sessions/session/start-ingest/")
            .method("POST")
            .header("X-API-Version", "1.0.0")
            .header(header::AUTHORIZATION, "Basic dGVzdHVzZXI6cGFzcw==")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let got: Value =
            serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(got["share_id"], "share-123");
        assert_eq!(got["project"], "project");
        assert_eq!(got["session"], "session");
        assert_eq!(got["dest_path"], "/adhoc/project/session/");
        assert!(got.get("username").is_none());
        assert!(got.get("password").is_none());
        assert!(got.get("share").is_none());
    }

    #[tokio::test]
    async fn authenticated_local_api_rejects_missing_credentials() {
        let test_setup = TestSetup::start().await;
        let req = Request::builder()
            .uri("/api/projects/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
