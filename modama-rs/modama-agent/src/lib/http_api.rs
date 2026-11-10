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
    local_sign_in::{ClientSignIn, MaybeClientSignIn, SIGN_IN_COOKIE, expired_sign_in_cookie},
};

use modama_common::local_api_messages::{
    APIAgentIdentity, APICreateSessionRequest, APICreateSessionResponse, APICreateShareResponse,
    APICurrentForegroundShare, APIHealthCheckResponse, APIListProjectsResponse,
    APIListSessionsResponse, APILoginRequest, APILoginResponse, APIProjectSummary,
    APISessionSummary,
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
    Ok(Json(api_state.fan_out.foreground_for(scope)?))
}

#[utoipa::path(
    get,
    path = "/api/identity/",
    operation_id = "get_identity",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses((status = 200, description = "Local agent identity", body = APIAgentIdentity))
)]
async fn api_identity(State(api_state): State<ApiState>) -> Json<APIAgentIdentity> {
    Json((&api_state.config.identity).into())
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
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    let share = api_state
        .actions()
        .start_ingest(&mut sign_in, &project_name, &session_name)
        .await?;
    Ok(sign_in.respond(Json(APICreateShareResponse {
        share_id: share.share_id,
        project: share.project,
        session: share.session,
        dest_path: share.dest_path,
    })))
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
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    api_state
        .actions()
        .stop_ingest(&mut sign_in, &share_id)
        .await?;
    Ok(sign_in.respond(StatusCode::NO_CONTENT))
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
    State(api_state): State<ApiState>,
    Json(request): Json<APILoginRequest>,
) -> Result<Response, LocalAPIError> {
    let (username, sign_in) = api_state
        .mgmt()
        .await?
        .login(&request.username, &request.password)
        .await?;
    Ok(sign_in.respond(Json(APILoginResponse { username })))
}

#[utoipa::path(
    post,
    path = "/api/logout/",
    operation_id = "logout",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses((status = 204, description = "Signed out; the session cookie is removed"))
)]
async fn api_logout(
    State(api_state): State<ApiState>,
    MaybeClientSignIn(sign_in): MaybeClientSignIn,
) -> Response {
    if let Some(sign_in) = sign_in {
        let result = match api_state.mgmt().await {
            Ok(mgmt) => mgmt.logout(&sign_in).await.map_err(LocalAPIError::from),
            Err(error) => Err(error),
        };
        // the sign-in is gone here either way; the management API's cookie just expires
        if let Err(error) = result {
            warn!("signing out at the management API failed: {error}");
        }
    }
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, expired_sign_in_cookie())],
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
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    let projects = api_state.mgmt().await?.projects(&mut sign_in).await?;
    let response = Json(APIListProjectsResponse {
        projects: projects
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
    Ok(sign_in.respond(response))
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
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
) -> Result<Response, LocalAPIError> {
    let details = api_state
        .mgmt()
        .await?
        .project(&mut sign_in, &project_name)
        .await?;
    let response = Json(APIListSessionsResponse {
        project: project_name,
        sessions: details
            .sessions
            .iter()
            .map(|session| session.name.clone())
            .collect(),
        session_summaries: details
            .sessions
            .into_iter()
            .map(|session| APISessionSummary {
                name: session.name,
                modified_at: session.mtime as i64,
            })
            .collect(),
    });
    Ok(sign_in.respond(response))
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
    State(api_state): State<ApiState>,
    ClientSignIn(mut sign_in): ClientSignIn,
    Json(create_session_req): Json<APICreateSessionRequest>,
) -> Result<Response, LocalAPIError> {
    api_state
        .actions()
        .create_session(&mut sign_in, &project_name, &create_session_req.session)
        .await?;
    Ok(sign_in.respond(Json(APICreateSessionResponse {
        project: project_name,
        session: create_session_req.session,
    })))
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
    if api_state.fan_out.connected_to_smbdrop() {
        (StatusCode::OK, Json(APIHealthCheckResponse::Ok))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(APIHealthCheckResponse::NotAvailable),
        )
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
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(SIGN_IN_COOKIE))),
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
