use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
};
use hyper::StatusCode;
use log::warn;
use std::sync::Arc;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use versioned::{VersionedRouterBuilder, semver::Version, versioned_route, versions};

use crate::{
    caller_scope::ConnInfo,
    local_http_service::{ApiState, LocalAPIError},
    local_session::{
        CallerSession, MaybeCallerSession, SESSION_COOKIE, expired_session_cookie,
        with_refreshed_session,
    },
};

use modama_common::local_api_messages::{
    APIAgentIdentity, APICreateSessionRequest, APICreateSessionResponse, APICreateShareResponse,
    APICurrentForegroundShare, APIHealthCheckResponse, APIListProjectsResponse,
    APIListSessionsResponse, APILoginRequest, APILoginResponse, APIProjectSummary,
};

#[utoipa::path(
    get,
    path = "/api/shares/foreground/",
    operation_id = "get_foreground_share",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses((status = 200, description = "The foreground ingest", body = APICurrentForegroundShare))
)]
/// Query for the foreground ingest
async fn api_shares_foreground(
    State(api_state): State<ApiState>,
    ConnectInfo(conn): ConnectInfo<ConnInfo>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let scope = api_state.resolve_scope(conn).await?;
    let fan_out = Arc::clone(&api_state.state.lock().await.fan_out);
    Ok(Json(fan_out.foreground_for(scope)?))
}

#[utoipa::path(
    get,
    path = "/api/identity/",
    operation_id = "get_identity",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses((status = 200, description = "Local agent identity", body = APIAgentIdentity))
)]
async fn api_identity(State(api_state): State<ApiState>) -> Json<APIAgentIdentity> {
    Json(match &api_state.state.lock().await.config.identity {
        crate::config::MTLSIdentity::Anonymous { hostname } => APIAgentIdentity::Anonymous {
            hostname: hostname.clone(),
        },
        crate::config::MTLSIdentity::AgentId {
            agent_name,
            resource_name,
        } => APIAgentIdentity::AgentId {
            agent_name: agent_name.clone(),
            resource_name: resource_name.clone(),
        },
    })
}

#[utoipa::path(
    post,
    path = "/api/projects/{project_name}/sessions/{session_name}/start-ingest/",
    operation_id = "start_ingest",
    params(
        ("X-API-Version" = String, Header, description = "API version"),
        ("project_name" = String, Path, description = "Project name"),
        ("session_name" = String, Path, description = "Session name")
    ),
    responses(
        (status = 200, description = "The ingest share: a new one, or the caller's existing one for this session", body = APICreateShareResponse),
        (status = 401, description = "Not signed in, or the session has ended"),
        (status = 403, description = "Not a member of the project"),
        (status = 404, description = "Project or session not found")
    ),
    security(("session_cookie" = []))
)]
async fn api_start_ingest(
    Path((project_name, session_name)): Path<(String, String)>,
    State(agent_state): State<ApiState>,
    CallerSession(session): CallerSession,
) -> Result<Response, LocalAPIError> {
    let sign_response = Arc::clone(&agent_state.token_provider)
        .fetch_token()
        .await?;
    let share = agent_state
        .mgmt()
        .await?
        .open_ingest_share(
            &session,
            &project_name,
            &session_name,
            &sign_response.resource,
            &sign_response.token,
        )
        .await?;
    let response = Json(APICreateShareResponse {
        share_id: share.value.share_id,
        project: share.value.project,
        session: share.value.session,
        dest_path: share.value.dest_path,
    });
    Ok(with_refreshed_session(share.refreshed, response))
}

#[utoipa::path(
    delete,
    path = "/api/shares/{share_id}/",
    operation_id = "delete_share",
    params(
        ("X-API-Version" = String, Header, description = "API version"),
        ("share_id" = String, Path, description = "Ingest share identifier")
    ),
    responses(
        (status = 204, description = "Ingest share stopped"),
        (status = 401, description = "Not signed in, or the session has ended"),
        (status = 403, description = "The ingest share belongs to someone else"),
        (status = 404, description = "Ingest share not found")
    ),
    security(("session_cookie" = []))
)]
async fn api_delete_share(
    Path(share_id): Path<String>,
    State(agent_state): State<ApiState>,
    CallerSession(session): CallerSession,
) -> Result<Response, LocalAPIError> {
    let closed = agent_state
        .mgmt()
        .await?
        .close_ingest_share(&session, &share_id)
        .await?;
    Ok(with_refreshed_session(
        closed.refreshed,
        StatusCode::NO_CONTENT,
    ))
}

#[utoipa::path(
    post,
    path = "/api/login/",
    operation_id = "login",
    params(("X-API-Version" = String, Header, description = "API version")),
    request_body = APILoginRequest,
    responses(
        (status = 200, description = "Signed in; the session is in the `modama_agent_session` cookie", body = APILoginResponse),
        (status = 401, description = "Wrong username or password")
    )
)]
async fn api_login(
    State(agent_state): State<ApiState>,
    Json(request): Json<APILoginRequest>,
) -> Result<Response, LocalAPIError> {
    let (username, session) = agent_state
        .mgmt()
        .await?
        .login(&request.username, &request.password)
        .await?;
    Ok(with_refreshed_session(
        Some(session),
        Json(APILoginResponse { username }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/logout/",
    operation_id = "logout",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses((status = 204, description = "Signed out; the session cookie is removed"))
)]
async fn api_logout(
    State(agent_state): State<ApiState>,
    MaybeCallerSession(session): MaybeCallerSession,
) -> Response {
    if let Some(session) = session {
        let result = match agent_state.mgmt().await {
            Ok(mgmt) => mgmt.logout(&session).await.map_err(LocalAPIError::from),
            Err(error) => Err(error),
        };
        // the session is gone here either way; the management API's cookie just expires
        if let Err(error) = result {
            warn!("signing out at the management API failed: {error}");
        }
    }
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, expired_session_cookie())],
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/api/projects/",
    operation_id = "list_projects",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses(
        (status = 200, description = "Available projects", body = APIListProjectsResponse),
        (status = 401, description = "Not signed in, or the session has ended")
    ),
    security(("session_cookie" = []))
)]
async fn api_list_projects(
    State(agent_state): State<ApiState>,
    CallerSession(session): CallerSession,
) -> Result<Response, LocalAPIError> {
    let projects = agent_state.mgmt().await?.projects(&session).await?;
    let response = Json(APIListProjectsResponse {
        projects: projects
            .value
            .projects
            .into_iter()
            .map(|project| APIProjectSummary {
                name: project.name,
                owner: project.owner,
                members: project.members,
                modified_at: project.mtime as i64,
            })
            .collect(),
    });
    Ok(with_refreshed_session(projects.refreshed, response))
}

#[utoipa::path(
    get,
    path = "/api/projects/{project_name}/sessions/",
    operation_id = "list_sessions_in_project",
    params(
        ("X-API-Version" = String, Header, description = "API version"),
        ("project_name" = String, Path, description = "Project name")
    ),
    responses(
        (status = 200, description = "Sessions in the project", body = APIListSessionsResponse),
        (status = 401, description = "Not signed in, or the session has ended"),
        (status = 404, description = "Project not found")
    ),
    security(("session_cookie" = []))
)]
async fn api_list_sessions_in_project(
    Path(project_name): Path<String>,
    State(agent_state): State<ApiState>,
    CallerSession(session): CallerSession,
) -> Result<Response, LocalAPIError> {
    let details = agent_state
        .mgmt()
        .await?
        .project(&session, &project_name)
        .await?;
    let response = Json(APIListSessionsResponse {
        project: project_name,
        sessions: details
            .value
            .sessions
            .into_iter()
            .map(|session| session.name)
            .collect(),
    });
    Ok(with_refreshed_session(details.refreshed, response))
}

#[utoipa::path(
    post,
    path = "/api/projects/{project_name}/sessions/",
    operation_id = "create_session_in_project",
    params(
        ("X-API-Version" = String, Header, description = "API version"),
        ("project_name" = String, Path, description = "Project name")
    ),
    request_body = APICreateSessionRequest,
    responses(
        (status = 200, description = "Created session", body = APICreateSessionResponse),
        (status = 401, description = "Not signed in, or the session has ended"),
        (status = 404, description = "Project not found")
    ),
    security(("session_cookie" = []))
)]
async fn api_create_session_in_project(
    Path(project_name): Path<String>,
    State(agent_state): State<ApiState>,
    CallerSession(session): CallerSession,
    Json(create_session_req): Json<APICreateSessionRequest>,
) -> Result<Response, LocalAPIError> {
    let created = agent_state
        .mgmt()
        .await?
        .create_session(&session, &project_name, &create_session_req.session)
        .await?;
    let response = Json(APICreateSessionResponse {
        project: project_name,
        session: create_session_req.session,
    });
    Ok(with_refreshed_session(created.refreshed, response))
}

#[utoipa::path(
    get,
    path = "/api/health/",
    operation_id = "health",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses(
        (status = 200, description = "Agent is healthy", body = APIHealthCheckResponse),
        (status = 503, description = "Agent is connecting and not yet available", body = APIHealthCheckResponse),
    )
)]
async fn api_health(State(api_state): State<ApiState>) -> impl IntoResponse {
    match api_state.state.lock().await.connection {
        crate::agent_state::ConnectionState::Connecting
        | crate::agent_state::ConnectionState::Authenticating => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(APIHealthCheckResponse::NotAvailable),
        ),
        crate::agent_state::ConnectionState::Connected => {
            (StatusCode::OK, Json(APIHealthCheckResponse::Ok))
        }
    }
}

fn latest_api_version() -> Version {
    Version::new(1, 0, 0)
}

fn api_routes() -> VersionedRouterBuilder<ApiState> {
    VersionedRouterBuilder::new()
        .route(
            versions!("1.0.0.."),
            versioned_route!(api_shares_foreground),
        )
        .route(versions!("1.0.0.."), versioned_route!(api_identity))
        .route(versions!("1.0.0.."), versioned_route!(api_delete_share))
        .route(versions!("1.0.0.."), versioned_route!(api_health))
        .route(versions!("1.0.0.."), versioned_route!(api_login))
        .route(versions!("1.0.0.."), versioned_route!(api_logout))
        .route(versions!("1.0.0.."), versioned_route!(api_list_projects))
        .route(
            versions!("1.0.0.."),
            versioned_route!(api_list_sessions_in_project),
        )
        .route(
            versions!("1.0.0.."),
            versioned_route!(api_create_session_in_project),
        )
        .route(versions!("1.0.0.."), versioned_route!(api_start_ingest))
}

fn customize_openapi(mut openapi: utoipa::openapi::OpenApi) -> utoipa::openapi::OpenApi {
    openapi.info.title = "modama-agent local API".to_owned();
    if let Some(components) = openapi.components.as_mut() {
        components.add_security_scheme(
            "session_cookie",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(SESSION_COOKIE))),
        );
    }
    openapi
}

/// Generate the canonical OpenAPI document for the local API.
pub fn openapi_document() -> Result<utoipa::openapi::OpenApi, LocalAPIError> {
    let api_routes = api_routes();
    Ok(customize_openapi(
        api_routes.generate_openapi(latest_api_version())?,
    ))
}

pub fn add_api_routes(router: Router<ApiState>, state: ApiState) -> Result<Router, LocalAPIError> {
    let api_routes = api_routes();
    let openapi = openapi_document()?;
    let versioned_router = api_routes.build(state.clone())?;
    let openapi = Arc::new(openapi);

    let router = router.with_state(state).merge(versioned_router);
    Ok(router.route(
        "/openapi.json",
        get(move || {
            let openapi = Arc::clone(&openapi);
            async move { Json((*openapi).clone()) }
        }),
    ))
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::openapi_document;

    #[test]
    fn checked_in_openapi_schema_matches_the_api() -> Result<(), Box<dyn std::error::Error>> {
        let checked_in: serde_json::Value =
            serde_json::from_str(include_str!("../../openapi.json"))?;
        let generated = serde_json::to_value(openapi_document()?)?;

        assert_eq!(checked_in, generated);
        Ok(())
    }
}
