//! Local minimal server-rendered ingest control UI.
//! Targets especially older browsers (Internet Explorer, ancient Firefox)
use axum::{
    Router,
    extract::{ConnectInfo, Form, Path, Query, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_macros::debug_handler;
use log::error;
use minijinja::{AutoEscape, Environment, Value, context};
use modama_common::{
    foreground::project_and_session,
    internal_api_types::{ConnectedAgent, ConnectedAgents},
    local_api_messages::APICurrentForegroundShare,
    messages::{AgentMountStatus, Share},
    tokens::{TokenError, TokenProvider},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use time::{Date, OffsetDateTime};
use tower_http::csrf::CsrfLayer;

use crate::{
    caller_scope::{ConnInfo, HelperScope, PlatformScopeResolver, ResolveScope, ScopeError},
    config::{AgentConfig, ConfigError},
    events_api::add_internal_api,
    http_api::add_api_routes,
    ingest_actions::{ActionError, IngestActions, Missing},
    local_fan_out::LocalFanOut,
    local_sign_in::{ClientSignIn, MaybeClientSignIn, expired_sign_in_cookie},
    mgmt_api::{MgmtApi, MgmtApiConnection, MgmtApiError, SignIn},
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
        "login.html" => Ok(Some(
            include_str!("templates/local_http/login.html").to_owned(),
        )),
        _ => Ok(None),
    });
    env.set_auto_escape_callback(|_| AutoEscape::Html);

    env
}

/// The agents connected at the resource of our token, for the HTML pages
#[derive(Serialize)]
struct ConnectedAgentsView {
    resource: String,
    own: ConnectedAgentView,
    others: Vec<ConnectedAgentView>,
}

impl From<ConnectedAgents> for ConnectedAgentsView {
    fn from(agents: ConnectedAgents) -> Self {
        Self {
            resource: agents.resource,
            own: agents.own.into(),
            others: agents.others.into_iter().map(Into::into).collect(),
        }
    }
}

/// A connected agent at our resource, with the mount status it reported
#[derive(Serialize)]
struct ConnectedAgentView {
    name: String,
    verified: bool,
    status: &'static str,
}

impl From<ConnectedAgent> for ConnectedAgentView {
    fn from(agent: ConnectedAgent) -> Self {
        Self {
            name: agent.name,
            verified: agent.verified,
            status: mount_status_label(agent.status),
        }
    }
}

/// How the HTML pages show a mount status; `?` if the agent didn't report one
fn mount_status_label(status: Option<AgentMountStatus>) -> &'static str {
    match status {
        Some(AgentMountStatus::Idle) => "idle",
        Some(AgentMountStatus::Reconciling) => "reconciling",
        Some(AgentMountStatus::LeftoverWork) => "reconciled, with work left over",
        Some(AgentMountStatus::NoHelper) => "no session helper",
        Some(AgentMountStatus::Unknown) | None => "?",
    }
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
struct OtherMountView {
    project: String,
    session: String,
    created_by: Option<String>,
    mount_path: String,
}

#[derive(Serialize)]
struct ForegroundView {
    kind: &'static str,
    project: Option<String>,
    session: Option<String>,
    message: Option<String>,
    created_by: Option<String>,
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
                created_by: None,
                close_href: None,
                refresh_status: false,
            },
            APICurrentForegroundShare::Unavailable {
                share_id,
                project,
                session,
                message,
                created_by,
                ..
            } => Self {
                kind: "unavailable",
                project: Some(project.clone()),
                session: Some(session.clone()),
                message: Some(message.clone()),
                created_by: created_by.clone(),
                close_href: Some(SafeLocalUrl::close_ingest(share_id)),
                refresh_status: true,
            },
            APICurrentForegroundShare::Mounted {
                share_id,
                project,
                session,
                created_by,
                ..
            } => Self {
                kind: "open",
                project: Some(project.clone()),
                session: Some(session.clone()),
                message: None,
                created_by: created_by.clone(),
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
    api_state: &ApiState,
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
        &api_state.jinja_env,
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

/// This agent's identity, and the agents connected at the resource of its token
fn local_snapshot(api_state: &ApiState) -> (String, Option<ConnectedAgentsView>) {
    (
        api_state.config.identity.to_string(),
        api_state.fan_out.connected_agents().map(Into::into),
    )
}

/// The desired foreground share; mounted or not, this is what the user can close, and what
/// prevents opening another ingest
fn desired_foreground(api_state: &ApiState) -> Option<Share> {
    api_state.fan_out.desired_foreground()
}

/// What the handlers of the local API server share
#[derive(Clone)]
pub struct ApiState {
    pub config: Arc<AgentConfig>,
    pub fan_out: Arc<LocalFanOut>,
    pub token_provider: Arc<dyn TokenProvider + Send + Sync>,
    pub jinja_env: Environment<'static>,
    pub scope_resolver: Arc<dyn ResolveScope>,
    pub mgmt_api: Arc<MgmtApi>,
}

impl ApiState {
    fn new(
        config: &Arc<AgentConfig>,
        fan_out: &Arc<LocalFanOut>,
        token_provider: &Arc<dyn TokenProvider + Send + Sync>,
    ) -> Self {
        Self {
            config: Arc::clone(config),
            fan_out: Arc::clone(fan_out),
            token_provider: Arc::clone(token_provider),
            jinja_env: create_env(),
            scope_resolver: Arc::new(PlatformScopeResolver),
            mgmt_api: Arc::new(MgmtApi::new()),
        }
    }

    /// Create sessions, open and stop ingest shares
    pub fn actions(&self) -> IngestActions<'_> {
        IngestActions {
            mgmt_api: &self.mgmt_api,
            mgmt_api_url: &self.config.mgmt_api_url,
            token_provider: &self.token_provider,
        }
    }

    /// mgmtinterface-api below the configured `mgmt_api_url`
    pub async fn mgmt(&self) -> Result<MgmtApiConnection, LocalAPIError> {
        Ok(self.mgmt_api.connect(&self.config.mgmt_api_url).await?)
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

    #[error("token error: {0}")]
    Token(#[from] TokenError),

    #[error("not signed in")]
    NotSignedIn,

    #[error("management API: {0}")]
    Mgmt(#[from] MgmtApiError),

    #[error("{0}")]
    Action(#[from] ActionError),

    #[error("template error: {0}")]
    Template(#[from] minijinja::Error),

    #[error("bind error: {0}")]
    Listen(#[from] std::io::Error),

    #[error("configuration: {0}")]
    Config(#[from] ConfigError),

    #[error("invalid API response: {0}")]
    InvalidResponse(#[from] modama_common::foreground::ShareDestinationError),

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
            LocalAPIError::NotSignedIn => action_error_response(ActionError::NotSignedIn),
            LocalAPIError::Mgmt(error) => action_error_response(error.into()),
            LocalAPIError::Token(error) => action_error_response(error.into()),
            LocalAPIError::Action(error) => action_error_response(error),
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

/// How the pages and the JSON API answer when something failed: the user isn't signed in,
/// the management API refused or couldn't be reached, or this agent has no token
fn action_error_response(error: ActionError) -> Response {
    match error {
        ActionError::NotSignedIn => (
            StatusCode::UNAUTHORIZED,
            [
                (header::SET_COOKIE, expired_sign_in_cookie()),
                (
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/plain"),
                ),
            ],
            "not signed in, or the session has ended",
        )
            .into_response(),
        ActionError::Forbidden { message } => (
            StatusCode::FORBIDDEN,
            [(header::CONTENT_TYPE, "text/plain")],
            message,
        )
            .into_response(),
        ActionError::NotFound { message, .. } => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            message,
        )
            .into_response(),
        // the request was refused, e.g. an invalid session name
        ActionError::Refused { status, message } => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
            [(header::CONTENT_TYPE, "text/plain")],
            message,
        )
            .into_response(),
        ActionError::Failed(error) => {
            error!("management API: {error}");
            (
                StatusCode::BAD_GATEWAY,
                [(header::CONTENT_TYPE, "text/plain")],
                error.to_string(),
            )
                .into_response()
        }
        ActionError::Token(token_error) => {
            error!("token error: {token_error}");
            // content-type? status code? could be internal server error in some cases, ...?
            (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "text/plain")],
                "bad request",
            )
                .into_response()
        }
    }
}

#[debug_handler]
async fn serve_index_page(
    State(api_state): State<ApiState>,
    ConnectInfo(conn): ConnectInfo<ConnInfo>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    let (identity, connected_agents) = local_snapshot(&api_state);
    let scope = api_state.resolve_scope(conn).await?;
    let foreground = api_state.fan_out.foreground_for(scope)?;
    let other_mounts = api_state.fan_out.other_mounts_for(scope);
    let other_mounts = other_mounts
        .into_iter()
        .map(|mount| OtherMountView {
            project: mount.project,
            session: mount.session,
            created_by: mount.created_by,
            mount_path: mount.mount_path,
        })
        .collect::<Vec<_>>();
    let mgmt = api_state.mgmt().await?;
    let username = mgmt.signed_in_user(&mut sign_in).await?;
    // the management API lists every project; offer the user's own
    let projects = mgmt
        .projects(&mut sign_in)
        .await?
        .projects
        .into_iter()
        .filter(|project| project.members.contains(&username) || project.owner == username)
        .map(|project| LinkItem {
            href: Some(SafeLocalUrl::start_project(&project.name)),
            name: project.name,
        })
        .collect::<Vec<_>>();
    let foreground = ForegroundView::new(&foreground);

    Ok(sign_in.respond(render_html(
        &api_state.jinja_env,
        "index.html",
        StatusCode::OK,
        context! {
            title => "modama agent",
            identity,
            username,
            connected_agents,
            foreground,
            other_mounts,
            projects,
        },
    )?))
}

/// The session names of a project
async fn session_names(
    api_state: &ApiState,
    sign_in: &mut SignIn,
    project_name: &str,
) -> Result<Vec<String>, LocalAPIError> {
    let details = api_state
        .mgmt()
        .await?
        .project(sign_in, project_name)
        .await?;
    Ok(details
        .sessions
        .into_iter()
        .map(|session| session.name)
        .collect())
}

async fn start_ingest_step1(
    Path(project_name): Path<String>,
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    let session_names = session_names(&api_state, &mut sign_in, &project_name).await?;
    let session_date = today();
    let default_session_name = session_name_prefix(session_date);
    Ok(sign_in.respond(render_session_list(
        &api_state,
        StatusCode::OK,
        &project_name,
        &session_names,
        None,
        &default_session_name,
        session_date,
    )?))
}

async fn create_session(
    Path(project_name): Path<String>,
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
    Form(form): Form<CreateSessionForm>,
) -> Result<Response, LocalAPIError> {
    let session_names = session_names(&api_state, &mut sign_in, &project_name).await?;

    let session_name = match validate_session_name(&form.session_name) {
        Ok(session_name) => session_name,
        Err(message) => {
            return Ok(sign_in.respond(render_session_list(
                &api_state,
                StatusCode::BAD_REQUEST,
                &project_name,
                &session_names,
                Some(message),
                &form.session_name,
                today(),
            )?));
        }
    };

    let created = api_state
        .actions()
        .create_session(&mut sign_in, &project_name, &session_name)
        .await;
    match created {
        Ok(()) => {}
        // the management API refused the name
        Err(ActionError::Refused { message, .. }) => {
            let message = format!("Could not create the session: {message}");
            return Ok(sign_in.respond(render_session_list(
                &api_state,
                StatusCode::BAD_REQUEST,
                &project_name,
                &session_names,
                Some(&message),
                &session_name,
                today(),
            )?));
        }
        Err(error) => return Err(error.into()),
    }

    let confirmation_url = SafeLocalUrl::start_session(&project_name, &session_name);
    Ok(sign_in.respond(Redirect::to(confirmation_url.as_str())))
}

async fn start_ingest_step2(
    Path((project_name, session_name)): Path<(String, String)>,
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    let session_names = session_names(&api_state, &mut sign_in, &project_name).await?;
    if !session_names.contains(&session_name) {
        return Ok(sign_in.respond(message_page(
            &api_state.jinja_env,
            StatusCode::NOT_FOUND,
            "Session not found",
            "The selected session does not exist in this project.",
        )?));
    }

    let identity = &api_state.config.identity;
    // TODO: name the resource an anonymous agent inherits from its token
    // (`LocalFanOut::logged_in`), like the connected agents on the index page
    let res_name = identity.resource().unwrap_or(identity.name());
    let instrument = identity.resource().unwrap_or("this resource");
    // starting doesn't close the open share: say what happens to it
    let current = match desired_foreground(&api_state) {
        Some(share) => {
            let (project, session) = project_and_session(&share.dest_path)?;
            let own = match &share.created_by {
                Some(created_by) => {
                    let mgmt = api_state.mgmt().await?;
                    *created_by == mgmt.signed_in_user(&mut sign_in).await?
                }
                // smbdrop didn't record it: say it stays open, rather than guess
                None => true,
            };
            Some(context! {
                project,
                session,
                created_by => share.created_by,
                own,
                close_href => SafeLocalUrl::close_ingest(&share.share_id),
            })
        }
        None => None,
    };
    let action = SafeLocalUrl::start_session(&project_name, &session_name);
    Ok(sign_in.respond(render_html(
        &api_state.jinja_env,
        "start_confirm.html",
        StatusCode::OK,
        context! {
            title => "Open ingest",
            project => project_name,
            session => session_name,
            resource => res_name,
            instrument,
            current,
            action,
        },
    )?))
}

async fn start_ingest_step3(
    Path((project_name, session_name)): Path<(String, String)>,
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    let started = api_state
        .actions()
        .start_ingest(&mut sign_in, &project_name, &session_name)
        .await;
    match started {
        Ok(_) => {}
        Err(ActionError::NotFound {
            what: Missing::Session,
            ..
        }) => {
            return Ok(sign_in.respond(message_page(
                &api_state.jinja_env,
                StatusCode::NOT_FOUND,
                "Session not found",
                "The selected session does not exist in this project.",
            )?));
        }
        Err(error) => return Err(error.into()),
    }

    // there's a good chance the ingest share is available within 300ms; with this delay
    // the user doesn't have to refresh the overview page (hopefully in most cases!)
    tokio::time::sleep(Duration::from_millis(300)).await;

    Ok(sign_in.respond(Redirect::to("/")))
}

async fn close_ingest_confirmation(
    Path(share_id): Path<String>,
    State(api_state): State<ApiState>,
    _session: ClientSignIn,
) -> Result<Response, LocalAPIError> {
    let Some(foreground) =
        desired_foreground(&api_state).filter(|share| share.share_id == share_id)
    else {
        return message_page(
            &api_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest cannot be closed",
            "This ingest is not the currently open foreground ingest.",
        );
    };
    let (project, session) = project_and_session(&foreground.dest_path)?;
    let action = SafeLocalUrl::close_ingest(&share_id);

    render_html(
        &api_state.jinja_env,
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
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    if desired_foreground(&api_state).is_none_or(|share| share.share_id != share_id) {
        return message_page(
            &api_state.jinja_env,
            StatusCode::CONFLICT,
            "Ingest cannot be closed",
            "This ingest is not the currently open foreground ingest.",
        );
    }

    api_state
        .actions()
        .stop_ingest(&mut sign_in, &share_id)
        .await?;
    Ok(sign_in.respond(Redirect::to("/")))
}

#[derive(Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
    next: Option<String>,
}

/// Where to go after signing in: a path on this server, or the overview. Anything else (another
/// origin, odd characters) could make the login an open redirect, so it's dropped. That includes
/// `//` and dot segments anywhere: `/.//evil.example` stays on this host, but browsers normalize
/// it to the path `//evil.example`, which a later redirect built from the path would send away.
fn local_next(next: Option<&str>) -> &str {
    next.filter(|next| {
        let path = next.split_once('?').map_or(*next, |(path, _query)| path);
        next.starts_with('/')
            && !next.contains("//")
            && !path.split('/').any(is_dot_segment)
            && next.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'/' | b'-' | b'_' | b'.' | b'~' | b'%' | b'?' | b'=' | b'&'
                    )
            })
    })
    .unwrap_or("/")
}

/// `.` or `..`, also percent-encoded: the WHATWG URL parser treats `%2e` like `.` here
fn is_dot_segment(segment: &str) -> bool {
    let segment = segment.to_ascii_lowercase();
    matches!(
        segment.as_str(),
        "." | "%2e" | ".." | ".%2e" | "%2e." | "%2e%2e"
    )
}

fn login_page(
    api_state: &ApiState,
    status: StatusCode,
    next: &str,
    username: &str,
    error: Option<&str>,
) -> Result<Response, LocalAPIError> {
    render_html(
        &api_state.jinja_env,
        "login.html",
        status,
        context! { title => "Sign in", next, username, error },
    )
}

async fn show_login(
    State(api_state): State<ApiState>,
    Query(query): Query<LoginQuery>,
) -> Result<Response, LocalAPIError> {
    login_page(
        &api_state,
        StatusCode::OK,
        local_next(query.next.as_deref()),
        "",
        None,
    )
}

async fn login(
    State(api_state): State<ApiState>,
    Form(form): Form<LoginForm>,
) -> Result<Response, LocalAPIError> {
    let next = local_next(form.next.as_deref());
    match api_state
        .mgmt()
        .await?
        .login(&form.username, &form.password)
        .await
    {
        Ok((_username, sign_in)) => Ok(sign_in.respond(Redirect::to(next))),
        Err(MgmtApiError::Unauthorized) => login_page(
            &api_state,
            StatusCode::UNAUTHORIZED,
            next,
            &form.username,
            Some("Wrong username or password."),
        ),
        Err(error) => Err(error.into()),
    }
}

async fn logout(
    State(api_state): State<ApiState>,
    MaybeClientSignIn(sign_in): MaybeClientSignIn,
) -> Response {
    if let Some(sign_in) = sign_in {
        let result = match api_state.mgmt().await {
            Ok(mgmt) => mgmt.logout(&sign_in).await.map_err(LocalAPIError::from),
            Err(error) => Err(error),
        };
        // signed out here either way; the management API's cookie just expires
        if let Err(error) = result {
            log::warn!("signing out at the management API failed: {error}");
        }
    }
    (
        [(header::SET_COOKIE, expired_sign_in_cookie())],
        Redirect::to("/login/"),
    )
        .into_response()
}

/// On the pages, "not signed in" (no cookie, or the management API ended the sign-in) leads to
/// the login page, which comes back here afterwards
async fn redirect_unauthorized_to_login(request: axum::extract::Request, next: Next) -> Response {
    let target = request
        .uri()
        .path_and_query()
        .map_or_else(|| "/".to_owned(), |target| target.as_str().to_owned());
    let response = next.run(request).await;
    if response.status() != StatusCode::UNAUTHORIZED {
        return response;
    }
    let location = format!(
        "/login/?next={}",
        utf8_percent_encode(local_next(Some(&target)), NON_ALPHANUMERIC)
    );
    let mut redirect = Redirect::to(&location).into_response();
    for cookie in response.headers().get_all(header::SET_COOKIE) {
        redirect
            .headers_mut()
            .append(header::SET_COOKIE, cookie.clone());
    }
    redirect
}

pub async fn serve_local_web_api(
    config: Arc<AgentConfig>,
    fan_out: Arc<LocalFanOut>,
    listen_addr: &str,
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
) -> Result<(), LocalAPIError> {
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    serve_local_web_api_on(listener, config, fan_out, token_provider).await
}

pub async fn serve_local_web_api_on(
    listener: tokio::net::TcpListener,
    config: Arc<AgentConfig>,
    fan_out: Arc<LocalFanOut>,
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
) -> Result<(), LocalAPIError> {
    let app = make_app(ApiState::new(&config, &fan_out, &token_provider))?;

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
        .route("/close-ingest/{share_id}/", post(close_ingest))
        // all routes above this get redirected to login without a valid session:
        .route_layer(middleware::from_fn(redirect_unauthorized_to_login))
        .route("/login/", get(show_login).post(login))
        .route("/logout/", post(logout));

    let router = add_internal_api(router)?;

    Ok(add_api_routes(router, state)?.layer(CsrfLayer::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MTLSIdentity;
    use crate::local_fan_out::{HelperPresence, LocalFanOut};
    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use modama_common::internal_api_types::{MountedShare, SessionHelperState};
    use modama_common::messages::Share as MessageShare;
    use modama_common::messages::{ConnectionId, Participant, ParticipantIdentity};
    use modama_common::tokens::{BoxFutureToken, TokenSignResponse};
    use rustls::ClientConfig;
    use serde_json::Value;
    use std::path::PathBuf;
    use tokio::sync::Mutex;
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

    /// What smbdrop sent last
    #[derive(Default)]
    struct SmbdropSays {
        foreground: Option<MessageShare>,
        shares: Vec<MessageShare>,
        participants: Vec<Participant>,
    }

    struct TestSetup {
        pub app: Router,
        fan_out: Arc<LocalFanOut>,
        /// The management API the agent is pointed at, until a test takes it to add mocks
        mgmt_server: Mutex<Option<mockito::ServerGuard>>,
        smbdrop: Mutex<SmbdropSays>,
        /// The session helper of the test scope, once it reported
        helper: Mutex<Option<HelperPresence>>,
    }

    impl TestSetup {
        pub async fn start() -> Self {
            Self::start_as(crate::config::MTLSIdentity::AgentId {
                agent_name: "agent-name".to_string(),
                resource_name: "res".to_string(),
            })
            .await
        }

        pub async fn start_as(identity: MTLSIdentity) -> Self {
            let root = RootCertStore::empty();
            let mgmt_server = mockito::Server::new_async().await;

            let cfg = crate::config::AgentConfig {
                events_api_url: Url::parse("http://localhost/").unwrap(),
                mgmt_api_url: Url::parse(&mgmt_server.url()).unwrap(),
                smb_hostname: "host".to_string(),
                smb_ip: tokio::sync::OnceCell::new(),
                smb_port: 445,
                smb_domain: "WINDOWS_FOR_WORKGROUPS".to_string(),
                identity,
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
            let cfg = Arc::new(cfg);
            let fan_out = Arc::new(LocalFanOut::new(cfg.identity.clone()));
            fan_out.logged_in(modama_common::agent::AgentId {
                resource: "res".to_string(),
                agent: "agent-name".to_string(),
                san_encoded: "x-modama-agent://res#agent-name".to_string(),
            });
            fan_out.welcomed(ConnectionId(1));

            let agent_at = |agent: &str| modama_common::agent::AgentId {
                resource: "res".to_string(),
                agent: agent.to_string(),
                san_encoded: format!("x-modama-agent://res#{agent}"),
            };
            let participant = |id, identity, status| Participant {
                connection: ConnectionId(id),
                identity,
                peer: format!("192.0.2.{id}:50000").parse().unwrap(),
                status,
            };
            let certified = |agent| ParticipantIdentity::Certified {
                agent: agent_at(agent),
            };
            // this agent; one that reported its status, and two that didn't
            let participants = vec![
                participant(1, certified("agent-name"), None),
                participant(2, certified("agent"), Some(AgentMountStatus::LeftoverWork)),
                participant(3, certified("old-agent"), None),
                participant(
                    4,
                    ParticipantIdentity::Anonymous {
                        name: "neighbour-pc".to_string(),
                        lender: agent_at("agent-name"),
                    },
                    None,
                ),
            ];

            let token_provider: Arc<dyn TokenProvider + Sync + Send> = Arc::new(DummyTokenProvider);
            let mut api_state = ApiState::new(&cfg, &fan_out, &token_provider);
            api_state.scope_resolver = Arc::new(TestScope);
            let app = make_app(api_state)
                .unwrap()
                .layer(MockConnectInfo(ConnInfo {
                    local: Some(([127, 0, 0, 1], 3001).into()),
                    peer: ([127, 0, 0, 1], 50000).into(),
                }));

            let test_setup = Self {
                app,
                fan_out,
                mgmt_server: Mutex::new(Some(mgmt_server)),
                smbdrop: Mutex::new(SmbdropSays::default()),
                helper: Mutex::new(None),
            };
            test_setup
                .smbdrop_sends(|says| says.participants = participants)
                .await;
            test_setup
        }

        /// smbdrop sends what it said last, changed by `change`
        async fn smbdrop_sends(&self, change: impl FnOnce(&mut SmbdropSays)) {
            let mut says = self.smbdrop.lock().await;
            change(&mut says);
            self.fan_out.apply_share_list(
                says.shares.clone(),
                says.foreground.clone(),
                says.participants.clone(),
            );
        }

        /// The session helper of the test scope reports `mounted`, as (share name, mount path)
        async fn helper_reports(&self, mounted: &[(&str, &str)]) {
            let mut helper = self.helper.lock().await;
            let helper = helper.get_or_insert_with(|| self.fan_out.connect_helper(TEST_SCOPE).0);
            helper.report(SessionHelperState {
                mounted_shares: mounted
                    .iter()
                    .map(|(share_name, mount_path)| MountedShare {
                        share_name: (*share_name).to_owned(),
                        mount_path: Some(PathBuf::from(mount_path)),
                    })
                    .collect(),
                errors: vec![],
                reconciled_share_ids: None,
            });
        }

        /// Make `share` the desired foreground share; if `mount_path` is given, the session
        /// helper of the test scope reports it as mounted there
        async fn set_foreground(&self, share: MessageShare, mount_path: Option<&str>) {
            if let Some(mount_path) = mount_path {
                self.helper_reports(&[(&share.share_name, mount_path)])
                    .await;
            }
            self.smbdrop_sends(|says| {
                says.shares = vec![share.clone()];
                says.foreground = Some(share);
            })
            .await;
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
            created_by: None,
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

            // past the CSRF check; without a session, on to the login page
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
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

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    /// A project as mgmtinterface-api lists it
    fn project_json(name: &str, owner: &str, members: &[&str]) -> Value {
        serde_json::json!({
            "relpath": format!("adhoc/{name}"), "name": name, "owner": owner,
            "members": members, "ctime": 0.0, "mtime": 0.0, "gid": 1001, "usage_bytes": null,
        })
    }

    /// `the-project` with these sessions, as mgmtinterface-api answers `GET projects/{p}`
    fn project_details_json(session_names: &[&str]) -> String {
        let mut details = project_json("the-project", "testuser", &["testuser"]);
        details["sessions"] = session_names
            .iter()
            .map(|name| {
                serde_json::json!({"name": name, "immutable": false, "ctime": 0.0, "mtime": 0.0})
            })
            .collect();
        details.to_string()
    }

    async fn mock_project_details(server: &mut mockito::ServerGuard, session_names: &[&str]) {
        server
            .mock("GET", "/mgmt-api/projects/the-project")
            .match_header("cookie", MGMT_COOKIE)
            .with_header("content-type", "application/json")
            .with_body(project_details_json(session_names))
            .create_async()
            .await;
    }

    /// A signed-in request for an HTML page
    fn page_request(method: &str, uri: &str, form: Option<&str>) -> Request<Body> {
        let request = Request::builder()
            .uri(uri)
            .method(method)
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE));
        match form {
            Some(form) => request
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.to_owned())),
            None => request.body(Body::empty()),
        }
        .unwrap()
    }

    async fn body_text(resp: axum::response::Response) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
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
            created_by: None,
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
                .contains("modama-agent-gui session-helper")
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

    /// The status and body of `/api/health/`
    async fn health(app: &Router) -> (StatusCode, Value) {
        let req = Request::builder()
            .uri("/api/health/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn health_is_ok_once_smbdrop_welcomed_us() {
        let test_setup = TestSetup::start().await;

        let (status, body) = health(&test_setup.app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["type"], "Ok");
    }

    #[tokio::test]
    async fn health_is_unavailable_until_smbdrop_welcomes_us() {
        let test_setup = TestSetup::start().await;
        // the steps of the message loop's next connection
        test_setup.fan_out.smbdrop_disconnected();
        let (status, body) = health(&test_setup.app).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["type"], "NotAvailable");

        test_setup.fan_out.logged_in(modama_common::agent::AgentId {
            resource: "res".to_string(),
            agent: "agent-name".to_string(),
            san_encoded: "x-modama-agent://res#agent-name".to_string(),
        });
        let (status, _) = health(&test_setup.app).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

        test_setup.fan_out.welcomed(ConnectionId(5));
        let (status, body) = health(&test_setup.app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["type"], "Ok");
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
            created_by: None,
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
        assert!(paths.contains_key("/api/logout/"));
        assert_eq!(
            got["components"]["securitySchemes"]["session_cookie"],
            serde_json::json!({"type": "apiKey", "in": "cookie", "name": "modama_agent_session"})
        );
    }

    #[tokio::test]
    async fn test_index_page_shows_connected_agents_and_projects() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("GET", "/mgmt-api/projects")
            .match_header("cookie", MGMT_COOKIE)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({"projects": [
                    project_json("the-project", "testuser", &["testuser"]),
                    project_json("not-mine", "someone", &["else"]),
                ]})
                .to_string(),
            )
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request("GET", "/", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );

        let body_str = body_text(resp).await;
        assert!(body_str.contains("<h2>Connected agents at res</h2>"));
        assert!(
            body_str
                .contains("<li>agent-name (this agent, identity verified): no session helper</li>")
        );
        // one agent reported its status, the other one didn't
        assert!(
            body_str
                .contains("<li>agent (identity verified): reconciled, with work left over</li>")
        );
        assert!(body_str.contains("<li>old-agent (identity verified): ?</li>"));
        assert!(body_str.contains("<li>neighbour-pc: ?</li>"));
        assert!(!body_str.contains("No other agents are connected."));
        assert!(body_str.contains("Signed in as testuser."));
        assert!(body_str.contains("<form action=\"/logout/\" method=\"post\">"));
        assert!(body_str.contains("the-project"));
        assert!(!body_str.contains("not-mine"));
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

    /// The index page as `testuser`, with no projects
    async fn index_page(test_setup: TestSetup) -> String {
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("GET", "/mgmt-api/projects")
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"projects": []}).to_string())
            .create_async()
            .await;
        let resp = test_setup
            .app
            .oneshot(page_request("GET", "/", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        body_text(resp).await
    }

    #[tokio::test]
    async fn index_page_says_when_no_other_agents_are_connected() {
        let test_setup = TestSetup::start().await;
        test_setup
            .smbdrop_sends(|says| says.participants.truncate(1))
            .await;
        let body_str = index_page(test_setup).await;
        assert!(
            body_str
                .contains("<li>agent-name (this agent, identity verified): no session helper</li>")
        );
        assert!(body_str.contains("No other agents are connected."));
    }

    #[tokio::test]
    async fn index_page_says_when_the_connected_agents_are_unknown() {
        let test_setup = TestSetup::start().await;
        test_setup.fan_out.smbdrop_disconnected();
        let body_str = index_page(test_setup).await;
        assert!(body_str.contains("<h2>Connected agents</h2>"));
        assert!(body_str.contains("Not connected to the server yet."));
    }

    #[tokio::test]
    async fn an_anonymous_agent_lists_the_agents_at_the_resource_of_its_token() {
        let test_setup = TestSetup::start_as(MTLSIdentity::Anonymous {
            hostname: "workstation".to_owned(),
        })
        .await;
        let body_str = index_page(test_setup).await;
        assert!(body_str.contains("<h2>Connected agents at res</h2>"));
        assert!(body_str.contains("<li>workstation (this agent): no session helper</li>"));
        assert!(
            body_str
                .contains("<li>agent (identity verified): reconciled, with work left over</li>")
        );
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
            include_str!("templates/local_http/login.html"),
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

    #[test]
    fn unavailable_foreground_shows_the_message_and_instructs_to_reload() {
        let unavailable = APICurrentForegroundShare::Unavailable {
            share_id: "share-123".to_owned(),
            project: "the-project".to_owned(),
            session: "test-session".to_owned(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            message: "Waiting for the session helper to mount the ingest share.".to_owned(),
            created_by: Some("alice".to_owned()),
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
            assert!(rendered.contains("Waiting for the session helper to mount the ingest share."));
            assert!(rendered.contains("<a href=\"/\">Reload this page</a>"));
            assert!(rendered.contains("to refresh the status."));
            // an unavailable ingest can still be closed, e.g. when no helper is running:
            assert!(rendered.contains("href=\"/close-ingest/share%2D123/\""));
            assert!(rendered.contains("Started by: alice"));
        }
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
    async fn index_offers_close_and_starting_another_while_an_ingest_is_open() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "share-123").await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("GET", "/mgmt-api/projects")
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({"projects": [project_json("the-project", "testuser", &[])]})
                    .to_string(),
            )
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request("GET", "/", None))
            .await
            .unwrap();
        let body = body_text(resp).await;

        assert!(body.contains("Status: open"));
        assert!(body.contains("href=\"/close-ingest/share%2D123/\""));
        // another ingest can be started while this one is open
        assert!(body.contains("href=\"/start-ingest/the%2Dproject/\""));
        assert!(!body.contains("Reload this page"));
        assert!(!body.contains("secret-password"));
    }

    #[tokio::test]
    async fn test_ingest_step1() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &["test-session"]).await;

        let date_before_request = today();
        let resp = test_setup
            .app
            .oneshot(page_request("GET", "/start-ingest/the-project/", None))
            .await
            .unwrap();
        let date_after_request = today();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_str = body_text(resp).await;
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

    #[tokio::test]
    async fn create_session_redirects_to_start_confirmation() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &["existing"]).await;
        let create = server
            .mock("POST", "/mgmt-api/projects/the-project/sessions")
            .match_header("cookie", MGMT_COOKIE)
            .match_body(mockito::Matcher::Json(
                serde_json::json!({"name": "new-session"}),
            ))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"name": "new-session", "immutable": false, "ctime": 1.0, "mtime": 1.0}"#)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "POST",
                "/start-ingest/the-project/",
                Some("session_name=++new-session++"),
            ))
            .await
            .unwrap();

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
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &["existing"]).await;
        let create = server
            .mock("POST", "/mgmt-api/projects/the-project/sessions")
            .expect(0)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "POST",
                "/start-ingest/the-project/",
                Some("session_name=bad%2Fname"),
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_text(resp).await;
        assert!(body.contains("Session name cannot contain a slash"));
        assert!(body.contains("value=\"bad&#x2f;name\""));
        assert!(body.contains("existing"));
        create.assert_async().await;
    }

    #[tokio::test]
    async fn session_creation_rejection_is_escaped_and_shown_inline() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &[]).await;
        server
            .mock("POST", "/mgmt-api/projects/the-project/sessions")
            .with_status(409)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code": "session_exists", "message": "session <exists>"}"#)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "POST",
                "/start-ingest/the-project/",
                Some("session_name=new-session"),
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_text(resp).await;
        assert!(body.contains("Could not create the session: session &lt;exists&gt;"));
        assert!(!body.contains("session <exists>"));
        assert!(body.contains("value=\"new-session\""));
    }

    #[tokio::test]
    async fn a_session_can_be_created_while_an_ingest_is_open() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "current-share").await;
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &[]).await;
        server
            .mock("POST", "/mgmt-api/projects/the-project/sessions")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"name": "new-session", "immutable": false, "ctime": 1.0, "mtime": 1.0}"#)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "POST",
                "/start-ingest/the-project/",
                Some("session_name=new-session"),
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    }

    #[tokio::test]
    async fn test_ingest_step2_invalid_session_name() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &["test-session"]).await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "GET",
                "/start-ingest/the-project/invalid-session/",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_ingest_step2_valid() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &["test-session"]).await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "GET",
                "/start-ingest/the-project/test-session/",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_str = body_text(resp).await;
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
        let mut server = mgmt_api_server(&test_setup).await;
        let mock = server
            .mock("GET", "/mgmt-api/projects/invalid-project")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code": "project_not_found", "message": "No such project."}"#)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "GET",
                "/start-ingest/invalid-project/test-session/",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_ingest_step3() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        let open = server
            .mock("POST", "/mgmt-api/ingest-shares")
            .match_header("cookie", MGMT_COOKIE)
            .match_header("x-agent-token", "dummy-token")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "project": "the-project", "session": "test-session", "resource": "resource",
            })))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"share_id": "share-123", "share_name": "share-123",
                    "project": "the-project", "session": "test-session",
                    "dest_path": "/adhoc/the-project/test-session/", "created_timestamp": 0,
                    "username": "share-user", "password": "share-pass", "uid": 1001,
                    "gid": 1001, "resource": "res", "created_by": "testuser",
                    "smb_host": "host", "workgroup": "W"}"#,
            )
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "POST",
                "/start-ingest/the-project/test-session/",
                None,
            ))
            .await
            .unwrap();
        open.assert_async().await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/");
    }

    #[tokio::test]
    async fn starting_an_unknown_session_shows_session_not_found() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        let open = server
            .mock("POST", "/mgmt-api/ingest-shares")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"code": "session_not_found",
                    "message": "There is no session named 'invalid-session'."}"#,
            )
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "POST",
                "/start-ingest/the-project/invalid-session/",
                None,
            ))
            .await
            .unwrap();
        open.assert_async().await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = body_text(resp).await;
        assert!(body.contains("Session not found"));
        assert!(body.contains("The selected session does not exist in this project."));
    }

    #[tokio::test]
    async fn starting_in_an_unknown_project_says_so() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("POST", "/mgmt-api/ingest-shares")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"code": "project_not_found",
                    "message": "There is no project named 'invalid-project'."}"#,
            )
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "POST",
                "/start-ingest/invalid-project/test-session/",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_text(resp).await,
            "There is no project named 'invalid-project'."
        );
    }

    #[tokio::test]
    async fn index_lists_the_other_mounted_shares() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "current-share").await;
        let older = MessageShare {
            share_name: "share-older".to_owned(),
            dest_path: "/adhoc/the-project/older-session/".to_owned(),
            created_by: Some("testuser".to_owned()),
            ..foreground_share("older-share")
        };
        test_setup
            .smbdrop_sends(|says| says.shares.insert(0, older))
            .await;
        test_setup
            .helper_reports(&[("share-name", "/mnt/ingest"), ("share-older", "/mnt/older")])
            .await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("GET", "/mgmt-api/projects")
            .with_header("content-type", "application/json")
            .with_body(r#"{"projects": []}"#)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request("GET", "/", None))
            .await
            .unwrap();
        let body = body_text(resp).await;
        assert!(body.contains("Also mounted on this computer"));
        assert!(
            body.contains("the-project / older-session, started by testuser: &#x2f;mnt&#x2f;older")
        );
    }

    #[tokio::test]
    async fn the_start_confirmation_says_what_happens_to_the_open_ingest() {
        // the user's own open ingest stays open
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "current-share").await;
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &["test-session"]).await;
        let resp = test_setup
            .app
            .oneshot(page_request(
                "GET",
                "/start-ingest/the-project/test-session/",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_text(resp).await;
        assert!(body.contains(
            "Your ingest share for session test-session in project the-project is still running"
        ));
        assert!(body.contains("href=\"/close-ingest/current%2Dshare/\""));

        // someone else's is detached from this resource
        let test_setup = TestSetup::start().await;
        test_setup
            .set_foreground(
                MessageShare {
                    created_by: Some("other.user".to_owned()),
                    ..foreground_share("current-share")
                },
                Some("/mnt/ingest"),
            )
            .await;
        let mut server = mgmt_api_server(&test_setup).await;
        mock_project_details(&mut server, &["test-session"]).await;
        let resp = test_setup
            .app
            .oneshot(page_request(
                "GET",
                "/start-ingest/the-project/test-session/",
                None,
            ))
            .await
            .unwrap();
        let body = body_text(resp).await;
        assert!(body.contains("other.user started the ingest share"));
        assert!(body.contains(
            "Starting this ingest makes it no longer available at res. Please coordinate with \
             other.user first."
        ));
        assert!(!body.contains("href=\"/close-ingest/"));
    }

    #[tokio::test]
    async fn an_ingest_can_be_opened_while_another_is_open() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "current-share").await;
        let mut server = mgmt_api_server(&test_setup).await;
        let open = server
            .mock("POST", "/mgmt-api/ingest-shares")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"share_id": "new-share", "share_name": "share-new", "project": "the-project",
                    "session": "test-session", "dest_path": "/adhoc/the-project/test-session/",
                    "created_timestamp": 0, "username": "u", "password": "p", "uid": 1001,
                    "gid": 1001, "resource": "res", "created_by": "testuser",
                    "smb_host": "host", "workgroup": "W"}"#,
            )
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request(
                "POST",
                "/start-ingest/the-project/test-session/",
                None,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        open.assert_async().await;
    }

    #[tokio::test]
    async fn close_ingest_requires_confirmation_and_redirects_after_deletion() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "share-123").await;
        let mut server = mgmt_api_server(&test_setup).await;
        let delete = server
            .mock("DELETE", "/mgmt-api/ingest-shares/share-123")
            .match_header("cookie", MGMT_COOKIE)
            .with_status(204)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .clone()
            .oneshot(page_request("GET", "/close-ingest/share-123/", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_text(resp).await;
        assert!(body.contains("Project: the-project"));
        assert!(body.contains("Session: test-session"));
        assert!(body.contains("<form action=\"/close-ingest/share%2D123/\" method=\"post\""));
        assert!(!body.contains("secret-password"));

        let resp = test_setup
            .app
            .oneshot(page_request("POST", "/close-ingest/share-123/", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/");
        delete.assert_async().await;
    }

    #[tokio::test]
    async fn stale_close_link_does_not_delete_a_share() {
        let test_setup = TestSetup::start().await;
        set_open_foreground(&test_setup, "current-share").await;

        let resp = test_setup
            .app
            .oneshot(page_request("POST", "/close-ingest/stale-share/", None))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn pages_without_a_sign_in_lead_to_the_login_page_and_back() {
        let test_setup = TestSetup::start().await;
        let req = Request::builder()
            .uri("/start-ingest/the-project/")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "/login/?next=%2Fstart%2Dingest%2Fthe%2Dproject%2F"
        );
    }

    #[tokio::test]
    async fn a_page_whose_sign_in_ended_leads_to_the_login_page() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("GET", "/mgmt-api/projects/the-project")
            .with_status(401)
            .with_body(r#"{"code": "not_authenticated", "message": "Please sign in."}"#)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request("GET", "/start-ingest/the-project/", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert!(
            resp.headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("/login/?next=")
        );
        assert!(set_cookies(&resp)[0].contains("Max-Age=0"));
    }

    #[tokio::test]
    async fn the_login_form_signs_in_and_returns_to_the_page() {
        let test_setup = TestSetup::start().await;
        let resp = test_setup
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/login/?next=%2Fstart%2Dingest%2Fthe%2Dproject%2F")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_text(resp).await;
        assert!(body.contains("<form action=\"/login/\" method=\"post\">"));
        assert!(body.contains("value=\"&#x2f;start-ingest&#x2f;the-project&#x2f;\""));

        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("POST", "/mgmt-api/auth/login")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "username": "testuser", "password": "p&ss", "persistent": false,
            })))
            .with_header(
                "set-cookie",
                "modama_session=abc; HttpOnly; Path=/; SameSite=lax",
            )
            .with_body(r#"{"username": "testuser", "persistent": false}"#)
            .create_async()
            .await;
        let resp = test_setup
            .app
            .oneshot(
                Request::builder()
                    .uri("/login/")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "username=testuser&password=p%26ss&next=%2Fstart-ingest%2Fthe-project%2F",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "/start-ingest/the-project/"
        );
        assert!(set_cookies(&resp)[0].starts_with(&signed_in_cookie(MGMT_COOKIE)));
    }

    #[tokio::test]
    async fn a_wrong_password_shows_the_form_again() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("POST", "/mgmt-api/auth/login")
            .with_status(401)
            .with_body(r#"{"code": "bad_credentials", "message": "Wrong username or password."}"#)
            .create_async()
            .await;
        let resp = test_setup
            .app
            .oneshot(
                Request::builder()
                    .uri("/login/")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("username=te%3Cst&password=wrong&next=%2F"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(set_cookies(&resp).is_empty());
        let body = body_text(resp).await;
        assert!(body.contains("Wrong username or password."));
        assert!(body.contains("value=\"te&lt;st\""));
    }

    #[test]
    fn only_local_paths_are_followed_after_login() {
        assert_eq!(
            local_next(Some("/start-ingest/p%2Dq/")),
            "/start-ingest/p%2Dq/"
        );
        for foreign in [
            "https://attacker.example/",
            "//attacker.example/",
            "/\\attacker.example",
            "start-ingest/",
            "/a b",
            "/\"><script>",
            "/\t/attacker.example",
            "/a//b",
            "/.//attacker.example",
            "/%2e//attacker.example",
            "/a/./b",
            "/a/../b",
            "/a/%2E%2e/b",
            "/a/.%2e",
            "/a/..?x=1",
        ] {
            assert_eq!(local_next(Some(foreign)), "/", "{foreign}");
        }
        // dots inside a segment, and dot segments in the query, are fine; an encoded line break
        // stays literal in the Location header
        for local in ["/a/.b/c..d/", "/a/?x=..&y=.", "/%0d%0aSet-Cookie=x"] {
            assert_eq!(local_next(Some(local)), local);
        }
        assert_eq!(local_next(None), "/");
    }

    #[tokio::test]
    async fn signing_out_removes_the_cookie_and_shows_the_login_page() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        let logout = server
            .mock("POST", "/mgmt-api/auth/logout")
            .match_header("cookie", MGMT_COOKIE)
            .with_status(204)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(page_request("POST", "/logout/", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login/");
        assert!(set_cookies(&resp)[0].contains("Max-Age=0"));
        logout.assert_async().await;
    }

    const MGMT_COOKIE: &str = "modama_session=abc";

    /// The management API the agent is pointed at, mounted at /mgmt-api, with the user
    /// signed in as `testuser`
    async fn mgmt_api_server(test_setup: &TestSetup) -> mockito::ServerGuard {
        let mut server = test_setup
            .mgmt_server
            .lock()
            .await
            .take()
            .expect("the management API server was taken already");
        server
            .mock("GET", "/api/versions")
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/mgmt-api/versions")
            .with_body(r#"{"supported": [1], "latest": 1, "deprecated": []}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/mgmt-api/auth/session")
            .match_header("cookie", MGMT_COOKIE)
            .with_header("content-type", "application/json")
            .with_body(r#"{"username": "testuser", "persistent": false}"#)
            .create_async()
            .await;
        server
    }

    /// The `Cookie` header of a client signed in with the management API cookie `mgmt_cookie`
    fn signed_in_cookie(mgmt_cookie: &str) -> String {
        let set_cookie = crate::local_sign_in::sign_in_cookie(
            &crate::mgmt_api::SignIn::from_cookie(mgmt_cookie),
        );
        set_cookie
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }

    fn set_cookies(resp: &axum::response::Response) -> Vec<String> {
        resp.headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn local_api_login_sets_the_sign_in_cookie() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        let login = server
            .mock("POST", "/mgmt-api/auth/login")
            // a normal session, not "keep me logged in": agents run on shared computers
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "username": "testuser", "password": "pass", "persistent": false,
            })))
            .with_header(
                "set-cookie",
                "modama_session=abc; HttpOnly; Path=/; SameSite=lax",
            )
            .with_body(r#"{"username": "testuser", "persistent": false}"#)
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/login/")
            .method("POST")
            .header("X-API-Version", "1.0.0")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"username": "testuser", "password": "pass"}"#,
            ))
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let cookies = set_cookies(&resp);
        assert_eq!(cookies.len(), 1);
        assert!(cookies[0].starts_with(&signed_in_cookie(MGMT_COOKIE)));
        assert!(cookies[0].contains("HttpOnly"));
        assert!(cookies[0].contains("SameSite=Strict"));
        assert!(!cookies[0].contains("Max-Age"));
        let got: Value =
            serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(got["username"], "testuser");
        login.assert_async().await;
    }

    #[tokio::test]
    async fn local_api_lists_enriched_projects() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        let mock = server
            .mock("GET", "/mgmt-api/projects")
            .match_header("cookie", MGMT_COOKIE)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"projects": [{"relpath": "project", "name": "Project One",
                    "owner": "owner", "members": ["member"], "ctime": 10.0,
                    "mtime": 1782432123.75, "gid": 1000, "usage_bytes": null}]}"#,
            )
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/projects/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE))
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // the management API didn't renew the sign-in, so the agent sets no cookie:
        assert!(set_cookies(&resp).is_empty());
        let got: Value =
            serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(got["projects"][0]["name"], "Project One");
        assert_eq!(got["projects"][0]["owner"], "owner");
        assert_eq!(got["projects"][0]["members"][0], "member");
        assert_eq!(got["projects"][0]["modified_at"], 1_782_432_123_i64);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn local_api_lists_sessions_with_their_modification_time() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        let mock = server
            .mock("GET", "/mgmt-api/projects/p")
            .match_header("cookie", MGMT_COOKIE)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"name": "p", "owner": "o", "members": [], "ctime": 1.0, "mtime": 2.0,
                    "gid": 1, "relpath": "adhoc/p", "usage_bytes": null,
                    "sessions": [
                        {"name": "old", "immutable": false, "ctime": 1.0, "mtime": 1782000000.5},
                        {"name": "new", "immutable": false, "ctime": 1.0, "mtime": 1782432123.75}
                    ]}"#,
            )
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/projects/p/sessions/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE))
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let got: Value =
            serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(got["sessions"], serde_json::json!(["old", "new"]));
        assert_eq!(
            got["session_summaries"],
            serde_json::json!([
                {"name": "old", "modified_at": 1_782_000_000_i64},
                {"name": "new", "modified_at": 1_782_432_123_i64}
            ])
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn a_renewed_sign_in_is_passed_on_to_the_client() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("GET", "/mgmt-api/projects/p")
            .match_header("cookie", MGMT_COOKIE)
            .with_header(
                "set-cookie",
                "modama_session=def; HttpOnly; Path=/; SameSite=lax",
            )
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"name": "p", "owner": "o", "members": [], "ctime": 1.0, "mtime": 2.0,
                    "gid": 1, "relpath": "adhoc/p", "usage_bytes": null,
                    "sessions": [{"name": "s", "immutable": false, "ctime": 1.0, "mtime": 2.0}]}"#,
            )
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/projects/p/sessions/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE))
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let cookies = set_cookies(&resp);
        assert_eq!(cookies.len(), 1);
        assert!(cookies[0].starts_with(&signed_in_cookie("modama_session=def")));
    }

    #[tokio::test]
    async fn an_ended_sign_in_removes_the_cookie() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("GET", "/mgmt-api/projects")
            .with_status(401)
            .with_body(r#"{"code": "not_authenticated", "message": "Please sign in."}"#)
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/projects/")
            .method("GET")
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE))
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        // no browser login popup, but the stale cookie goes:
        assert!(resp.headers().get(header::WWW_AUTHENTICATE).is_none());
        assert!(set_cookies(&resp)[0].contains("Max-Age=0"));
    }

    #[tokio::test]
    async fn closing_someone_elses_share_is_forbidden_with_the_reason() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("DELETE", "/mgmt-api/ingest-shares/theirs")
            .with_status(403)
            .with_body(r#"{"code": "not_share_owner", "message": "Only the owner can close it."}"#)
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/shares/theirs/")
            .method("DELETE")
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE))
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"Only the owner can close it.");
    }

    #[tokio::test]
    async fn local_api_logout_signs_out_and_removes_the_cookie() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        let logout = server
            .mock("POST", "/mgmt-api/auth/logout")
            .match_header("cookie", MGMT_COOKIE)
            .with_status(204)
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/logout/")
            .method("POST")
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE))
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(set_cookies(&resp)[0].contains("Max-Age=0"));
        logout.assert_async().await;
    }

    #[tokio::test]
    async fn local_api_deletes_share_without_returning_management_body() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        let mock = server
            .mock("DELETE", "/mgmt-api/ingest-shares/share-123")
            .match_header("cookie", MGMT_COOKIE)
            .with_status(204)
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/shares/share-123/")
            .method("DELETE")
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE))
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
        let mut server = mgmt_api_server(&test_setup).await;
        let open = server
            .mock("POST", "/mgmt-api/ingest-shares")
            .match_header("x-agent-token", "dummy-token")
            .match_header("cookie", MGMT_COOKIE)
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"share_id": "share-123", "share_name": "share-name",
                    "project": "project", "session": "session",
                    "dest_path": "/adhoc/project/session/", "created_timestamp": 0,
                    "username": "temporary-user", "password": "temporary-secret",
                    "uid": 1000, "gid": 1000, "resource": "resource",
                    "created_by": "testuser", "smb_host": "host", "workgroup": "W"}"#,
            )
            .create_async()
            .await;
        let req = Request::builder()
            .uri("/api/projects/project/sessions/session/start-ingest/")
            .method("POST")
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE))
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
        open.assert_async().await;
    }

    /// A signed-in request to the JSON API
    fn api_request(method: &str, uri: &str, json: Option<&str>) -> Request<Body> {
        let request = Request::builder()
            .uri(uri)
            .method(method)
            .header("X-API-Version", "1.0.0")
            .header(header::COOKIE, signed_in_cookie(MGMT_COOKIE));
        match json {
            Some(json) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_owned())),
            None => request.body(Body::empty()),
        }
        .unwrap()
    }

    #[tokio::test]
    async fn local_api_creates_a_session_with_the_name_as_given() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        // the management API checks the name, not the agent
        let create = server
            .mock("POST", "/mgmt-api/projects/the-project/sessions")
            .match_header("cookie", MGMT_COOKIE)
            .match_body(mockito::Matcher::Json(
                serde_json::json!({"name": " new session "}),
            ))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"name": " new session ", "immutable": false, "ctime": 1.0, "mtime": 1.0}"#,
            )
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(api_request(
                "POST",
                "/api/projects/the-project/sessions/",
                Some(r#"{"session": " new session "}"#),
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let got: Value = serde_json::from_str(&body_text(resp).await).unwrap();
        assert_eq!(got["project"], "the-project");
        assert_eq!(got["session"], " new session ");
        create.assert_async().await;
    }

    #[tokio::test]
    async fn local_api_passes_a_refused_session_name_on() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("POST", "/mgmt-api/projects/the-project/sessions")
            .with_status(422)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code": "invalid_name", "message": "Not a valid session name."}"#)
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(api_request(
                "POST",
                "/api/projects/the-project/sessions/",
                Some(r#"{"session": "a/b"}"#),
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body_text(resp).await, "Not a valid session name.");
    }

    #[tokio::test]
    async fn local_api_start_for_an_unknown_session_is_not_found() {
        let test_setup = TestSetup::start().await;
        let mut server = mgmt_api_server(&test_setup).await;
        server
            .mock("POST", "/mgmt-api/ingest-shares")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"code": "session_not_found", "message": "There is no session named 's'."}"#,
            )
            .create_async()
            .await;

        let resp = test_setup
            .app
            .oneshot(api_request(
                "POST",
                "/api/projects/p/sessions/s/start-ingest/",
                None,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_text(resp).await, "There is no session named 's'.");
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
