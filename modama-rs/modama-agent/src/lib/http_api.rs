use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    typed_header::TypedHeaderRejection,
    TypedHeader,
};
use hyper::StatusCode;
use log::warn;

use crate::local_http_service::{get_auth, ApiState, LocalAPIError};

use modama_common::{
    local_api_messages::{
        APICreateSessionRequest, APICreateSessionResponse, APICreateShareResponse,
        APICurrentForegroundShare, APIListProjectsResponse, APIListSessionsResponse,
        APILoginResponse,
    },
    mgmt_client::MgmtClient,
};

// #[debug_handler]
async fn api_shares_foreground(
    State(api_state): State<ApiState>,
) -> Json<APICurrentForegroundShare> {
    Json(
        match &api_state.state.lock().await.current_shares.foreground_share {
            None => APICurrentForegroundShare::None,
            Some(s) => match s.mount_path.to_str() {
                None => {
                    let p = s.mount_path.to_string_lossy();
                    warn!("Mount path {p} contains invalid unicode, cannot get foreground share!");
                    APICurrentForegroundShare::None
                }
                Some(mount_path) => APICurrentForegroundShare::Share {
                    dest_path: s.share.dest_path.to_owned(),
                    mount_path: mount_path.to_owned(),
                },
            },
        },
    )
}

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

    // FIXME: `dest_path` should probably be "normalized", as it is currently
    // the raw path as it is used inside the smbdrop container
    Ok(Json(APICreateShareResponse { share }).into_response())
}

async fn api_login(
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let auth = get_auth(auth).await?;

    Ok(Json(APILoginResponse {
        username: auth.username().to_owned(),
    })
    .into_response())
}

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
            .map(|adhoc_project| adhoc_project.name.clone())
            .collect(),
    };

    Ok(Json(resp).into_response())
}

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

pub fn add_api_routes(router: Router<ApiState>) -> Router<ApiState> {
    // FIXME: make the URLs beautiful
    router
        .route("/api/shares/foreground/", get(api_shares_foreground))
        .route("/api/login/", post(api_login))
        .route("/api/projects/", get(api_list_projects))
        .route(
            "/api/projects/{project_name}/sessions/",
            get(api_list_sessions_in_project),
        )
        .route(
            "/api/projects/{project_name}/sessions/",
            post(api_create_session_in_project),
        )
        .route(
            "/api/projects/{project_name}/sessions/{session_name}/start-ingest/",
            post(api_start_ingest),
        )
}
