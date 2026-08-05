use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    typed_header::TypedHeaderRejection,
    TypedHeader,
};
use hyper::StatusCode;
use log::warn;
use std::sync::Arc;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use versioned::{semver::Version, versioned_route, versions, VersionedRouterBuilder};

use crate::local_http_service::{get_auth, ApiState, LocalAPIError};

use modama_common::{
    local_api_messages::{
        APIAgentIdentity, APICreateSessionRequest, APICreateSessionResponse,
        APICreateShareResponse, APICurrentForegroundShare, APIHealthCheckResponse,
        APIListProjectsResponse, APIListSessionsResponse, APILoginResponse, APIProjectSummary,
    },
    mgmt_client::MgmtClient,
};

fn project_and_session(dest_path: &str) -> Result<(String, String), String> {
    let mut components = dest_path.split('/').filter(|part| !part.is_empty()).rev();
    let session = components
        .next()
        .ok_or_else(|| format!("share destination has no session: {dest_path:?}"))?;
    let project = components
        .next()
        .ok_or_else(|| format!("share destination has no project: {dest_path:?}"))?;
    Ok((project.to_owned(), session.to_owned()))
}

#[utoipa::path(
    get,
    path = "/api/shares/foreground/",
    operation_id = "get_foreground_share",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses((status = 200, description = "Current foreground share", body = APICurrentForegroundShare))
)]
/// Query for the current foreground share
async fn api_shares_foreground(State(api_state): State<ApiState>) -> axum::response::Response {
    let response = match &api_state.state.lock().await.current_shares.foreground_share {
        None => APICurrentForegroundShare::None,
        Some(s) => match s.mount_path.to_str() {
            None => {
                let p = s.mount_path.to_string_lossy();
                warn!("Mount path {p} contains invalid unicode, cannot get foreground share!");
                APICurrentForegroundShare::None
            }
            Some(mount_path) => {
                let (project, session) = match project_and_session(&s.share.dest_path) {
                    Ok(parts) => parts,
                    Err(error) => return LocalAPIError::InvalidResponse(error).into_response(),
                };
                APICurrentForegroundShare::Share {
                    share_id: s.share.share_id.clone(),
                    project,
                    session,
                    dest_path: s.share.dest_path.to_owned(),
                    mount_path: mount_path.to_owned(),
                }
            }
        },
    };
    Json(response).into_response()
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
        (status = 200, description = "Created ingest share", body = APICreateShareResponse),
        (status = 401, description = "Authentication failed"),
        (status = 404, description = "Project or session not found")
    ),
    security(("basic_auth" = []))
)]
async fn api_start_ingest(
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

    let details = c.project_details(&project_name).await?;

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return Ok((StatusCode::NOT_FOUND).into_response());
    }

    let sign_response = agent_state.token_provider.fetch_token().await?;
    let share = c
        .create_share(
            &project_name,
            &session_name,
            &sign_response.resource,
            &sign_response.token,
        )
        .await?;

    let (project, session) =
        project_and_session(&share.dest_path).map_err(LocalAPIError::InvalidResponse)?;
    Ok(Json(APICreateShareResponse {
        share_id: share.share_id,
        project,
        session,
        dest_path: share.dest_path,
    })
    .into_response())
}

#[utoipa::path(
    delete,
    path = "/api/shares/{share_id}/",
    operation_id = "delete_share",
    params(
        ("X-API-Version" = String, Header, description = "API version"),
        ("share_id" = String, Path, description = "Share identifier")
    ),
    responses(
        (status = 204, description = "Share deletion accepted"),
        (status = 401, description = "Authentication failed"),
        (status = 404, description = "Share not found")
    ),
    security(("basic_auth" = []))
)]
async fn api_delete_share(
    Path(share_id): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<StatusCode, LocalAPIError> {
    let auth = get_auth(auth).await?;
    let client = MgmtClient::new(
        &agent_state.state.lock().await.config.mgmt_api_url,
        auth.username(),
        auth.password(),
    )?;
    client.delete_share(&share_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/login/",
    operation_id = "login",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses(
        (status = 200, description = "Authenticated user", body = APILoginResponse),
        (status = 401, description = "Authentication failed")
    ),
    security(("basic_auth" = []))
)]
async fn api_login(
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let auth = get_auth(auth).await?;

    Ok(Json(APILoginResponse {
        username: auth.username().to_owned(),
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/api/projects/",
    operation_id = "list_projects",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses(
        (status = 200, description = "Available projects", body = APIListProjectsResponse),
        (status = 401, description = "Authentication failed")
    ),
    security(("basic_auth" = []))
)]
async fn api_list_projects(
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let auth = get_auth(auth).await?;
    let c = MgmtClient::new(
        &agent_state.state.lock().await.config.mgmt_api_url,
        auth.username(),
        auth.password(),
    )?;

    let projects = c.list_projects().await?;

    let resp = APIListProjectsResponse {
        projects: projects
            .projects
            .iter()
            .map(|project| APIProjectSummary {
                name: project.name.clone(),
                owner: project.owner.clone(),
                members: project.members.clone(),
                modified_at: project.mtime as i64,
            })
            .collect(),
    };

    Ok(Json(resp).into_response())
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
        (status = 401, description = "Authentication failed"),
        (status = 404, description = "Project not found")
    ),
    security(("basic_auth" = []))
)]
async fn api_list_sessions_in_project(
    Path(project_name): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let auth = get_auth(auth).await?;
    let c = MgmtClient::new(
        &agent_state.state.lock().await.config.mgmt_api_url,
        auth.username(),
        auth.password(),
    )?;

    let details = c.project_details(&project_name).await?;

    let resp = APIListSessionsResponse {
        project: project_name.clone(),
        sessions: details.datasets.iter().map(|d| d.name.clone()).collect(),
    };

    Ok(Json(resp).into_response())
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
        (status = 401, description = "Authentication failed"),
        (status = 404, description = "Project not found")
    ),
    security(("basic_auth" = []))
)]
async fn api_create_session_in_project(
    Path(project_name): Path<String>,
    State(agent_state): State<ApiState>,
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
    Json(create_session_req): Json<APICreateSessionRequest>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let auth = get_auth(auth).await?;
    let c = MgmtClient::new(
        &agent_state.state.lock().await.config.mgmt_api_url,
        auth.username(),
        auth.password(),
    )?;

    c.create_dataset(&project_name, &create_session_req.session, auth.username())
        .await?;

    Ok(Json(APICreateSessionResponse {
        project: project_name.clone(),
        session: create_session_req.session.clone(),
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/api/health/",
    operation_id = "health",
    params(("X-API-Version" = String, Header, description = "API version")),
    responses((status = 200, description = "Agent is healthy", body = APIHealthCheckResponse))
)]
async fn api_health(State(_api_state): State<ApiState>) -> Json<APIHealthCheckResponse> {
    Json(APIHealthCheckResponse::Ok)
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
            "basic_auth",
            SecurityScheme::Http(Http::new(HttpAuthScheme::Basic)),
        );
    }
    openapi
}

/// Generate the canonical OpenAPI document for the local API.
pub fn openapi_document() -> Result<utoipa::openapi::OpenApi, versioned::RouteConfigError> {
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
