use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    typed_header::TypedHeaderRejection,
    TypedHeader,
};
use axum_macros::debug_handler;
use log::error;
use minijinja::{context, Environment};
use modama_common::{
    mgmt_client::{MgmtClient, MgmtError},
    tokens::{TokenError, TokenProvider},
};
use std::{error::Error, sync::Arc};
use tokio::sync::Mutex;

use crate::{agent_state::AgentState, config::ConfigError, http_api::add_api_routes};

fn create_env() -> Environment<'static> {
    let mut env = Environment::new();

    env.set_loader(|name| match name {
        "start_ingest_step2.html" => Ok(Some(
            "<html><form action='.' method='post'><input type='submit'></form></html>".to_owned(),
        )),
        _ => Ok(None),
    });

    env
}

/// Wrapper around the AgentState for the local API server
#[derive(Clone)]
pub struct ApiState {
    pub state: Arc<Mutex<AgentState>>,
    pub token_provider: Arc<dyn TokenProvider + Send + Sync>,
    pub jinja_env: Environment<'static>,
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

    #[error("configuration: {0}")]
    Config(#[from] ConfigError),
}

impl IntoResponse for LocalAPIError {
    fn into_response(self) -> axum::response::Response {
        match self {
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
                    loop {
                        let source = error.source();
                        if let Some(inner) = source {
                            error!("  error details: {:?}", inner);
                            error = inner;
                        }
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
    auth: Result<TypedHeader<Authorization<Basic>>, TypedHeaderRejection>,
) -> Result<impl IntoResponse, LocalAPIError> {
    let mut result = String::new();
    let auth = get_auth(auth).await?;

    let state = agent_state.state.lock().await;

    match &state.config.identity {
        crate::config::MTLSIdentity::Anonymous { hostname } => {
            result += &format!("This agent: Anonymous running on {hostname}",);
        }
        crate::config::MTLSIdentity::AgentId {
            agent_name,
            resource_name,
        } => {
            let matching_agents = state
                .connected_agents
                .iter()
                .filter(|a| a.resource == *resource_name);
            result += "Connected agents at this resource: ";
            for a in matching_agents {
                result += &(a.san_encoded.to_owned() + " ");
            }
            result += "\n";
            result += &format!("This agent: {} at {}", agent_name, resource_name);
        }
    }

    result += "\n";

    let c = MgmtClient::new(&state.config.mgmt_api_url, auth.username(), auth.password())?;

    let projects = c.list_projects().await?;

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

    let details = c.project_details(&project_name).await?;
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

    let details = c.project_details(&project_name).await?;

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return Ok((StatusCode::NOT_FOUND).into_response());
    }

    let tpl = agent_state
        .jinja_env
        .get_template("start_ingest_step2.html")?;
    let res_name = match &agent_state.state.lock().await.config.identity {
        crate::config::MTLSIdentity::Anonymous { hostname } => hostname.clone(),
        crate::config::MTLSIdentity::AgentId {
            agent_name: _,
            resource_name,
        } => resource_name.clone(),
    };
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

    let details = c.project_details(&project_name).await?;
    let mut resp = String::new();

    if !details.datasets.iter().any(|d| d.name == session_name) {
        return Ok((StatusCode::NOT_FOUND).into_response());
    }

    let sign_response = agent_state.token_provider.fetch_token().await?;
    c.create_share(
        &project_name,
        &session_name,
        &sign_response.resource,
        &sign_response.token,
    )
    .await?;

    resp += "share created!";

    Ok(resp.into_response())
}

pub async fn serve_local_web_api(
    state: Arc<Mutex<AgentState>>,
    listen_addr: &str,
    token_provider: Arc<dyn TokenProvider + Sync + Send>,
) -> Result<(), LocalAPIError> {
    let app = make_app(ApiState::new(&state, &token_provider));
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;

    #[allow(clippy::unwrap_used)] // infallible
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

fn make_app(state: ApiState) -> Router {
    let router = Router::new()
        .route("/", get(serve_index_page))
        .route("/start-ingest/{project_name}/", get(start_ingest_step1))
        .route(
            "/start-ingest/{project_name}/{session_name}/",
            get(start_ingest_step2),
        )
        .route(
            "/start-ingest/{project_name}/{session_name}/",
            post(start_ingest_step3),
        );

    add_api_routes(router)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share::MountedShare;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
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
                linux_config: None,
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
            let mut state_inner = crate::agent_state::AgentState::new(&cfg);

            state_inner
                .connected_agents
                .push(modama_common::agent::AgentId {
                    resource: "res".to_string(),
                    agent: "agent".to_string(),
                    san_encoded: "x-modama-agent://res#agent".to_string(),
                });

            let shared_state = Arc::new(Mutex::new(state_inner));

            let token_provider: Arc<dyn TokenProvider + Sync + Send> = Arc::new(DummyTokenProvider);
            let api_state = ApiState::new(&shared_state, &token_provider);
            let app = make_app(api_state);

            Self { app, shared_state }
        }
    }

    #[tokio::test]
    async fn test_shares_foreground_endpoint_returns_share() {
        let test_setup = TestSetup::start().await;

        let mshare = MountedShare {
            share: MessageShare {
                resource: None,
                share_id: "id".to_string(),
                share_name: "sname".to_string(),
                dest_path: "dest".to_string(),
                created_timestamp: 0,
                username: "u".to_string(),
                password: "p".to_string(),
                uid: 1000,
                gid: 1000,
            },
            mount_path: PathBuf::from("/mnt/point"),
        };
        test_setup
            .shared_state
            .lock()
            .await
            .current_shares
            .foreground_share = Some(mshare.clone());
        let req = Request::builder()
            .uri("/api/shares/foreground/")
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let got: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(got["type"], "Share");
        assert_eq!(got["dest_path"], "dest");
        assert_eq!(got["mount_path"], "/mnt/point");
    }

    #[tokio::test]
    async fn test_no_fg_share_by_default() {
        let test_setup = TestSetup::start().await;

        let req = Request::builder()
            .uri("/api/shares/foreground/")
            .method("GET")
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

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body_str.contains("x-modama-agent://res#agent"));
        assert!(body_str.contains("agent-name"));
        assert!(body_str.contains("the-project"));
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

        let resp = test_setup.app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();

        assert!(body_str.contains("test-session"));
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

        assert!(body_str.contains("<form action='.' method='post'"));
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
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();

        assert!(body_str.contains("share created!"));
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
}
