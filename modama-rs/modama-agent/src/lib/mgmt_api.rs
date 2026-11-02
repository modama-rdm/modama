//! mgmtinterface-api, as the agent uses it: finds where the API is mounted, and carries the
//! caller's session.
//!
//! The agent keeps no session of its own. A caller logs in once and gets an
//! [`UpstreamSession`] back (the API's session cookie, as a `Cookie` header value), which it
//! passes into every later call. The API re-issues the cookie now and then, which moves its idle
//! timeout; a call returns the re-issued session in [`Authed::refreshed`], and the caller has to
//! keep that one from then on.

use std::sync::Arc;

use log::{info, warn};
use modama_common::{install_rustls_crypto_provider, version::modama_git_version};
use reqwest::cookie::{CookieStore, Jar};
use tokio::sync::Mutex;
use url::Url;

use crate::mgmt_api_client::generated::{
    ApiOpError, CreateIngestShareRequest, CreateSessionRequest, HttpClient, IngestShare,
    LoginRequest, ProjectDetails, ProjectList,
};

/// The contract version (`X-Modama-API-Version`) this agent is built against
pub const API_VERSION: &str = "1";
const API_VERSION_NUMBER: i64 = 1;

/// Where mgmtinterface-api may be mounted below `mgmt_api_url`, in order of preference: `/api`
/// once it has replaced the legacy Flask API there, `/mgmt-api` until then, and the configured
/// URL itself for a config that already points at the API.
const MOUNT_POINTS: [&str; 3] = ["/api", "/mgmt-api", ""];

#[derive(Debug, thiserror::Error)]
pub enum MgmtApiError {
    #[error("the management API is unreachable: {0}")]
    Unreachable(String),

    #[error(
        "no management API that speaks version {API_VERSION} found at {server} (tried {tried})"
    )]
    NotFound { server: Url, tried: String },

    #[error("not signed in, or the session has ended")]
    Unauthorized,

    #[error("{message}")]
    Forbidden { code: String, message: String },

    /// The API reported something as missing (`code` says what, e.g. `session_not_found`)
    #[error("{message}")]
    Missing { code: String, message: String },

    /// A 404 without the API's error body: the route itself doesn't exist (anymore)
    #[error("the management API has no route for this request")]
    NoRoute,

    #[error("the management API refused the request ({status}): {message}")]
    Backend { status: u16, message: String },

    #[error("invalid response from the management API: {0}")]
    InvalidResponse(String),
}

impl MgmtApiError {
    /// Whether the mount point found earlier may be wrong now, so it should be looked up again
    fn invalidates_discovery(&self) -> bool {
        matches!(self, Self::Unreachable(_) | Self::NoRoute)
    }
}

/// The caller's session with mgmtinterface-api: the value of the `Cookie` header to send.
/// Opaque to everyone but this module.
#[derive(Clone, PartialEq, Eq)]
pub struct UpstreamSession(String);

impl UpstreamSession {
    pub fn from_token(token: &str) -> Self {
        Self(token.to_owned())
    }

    pub fn token(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for UpstreamSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UpstreamSession([REDACTED])")
    }
}

/// The answer to an authenticated call, and the session to use from now on if the API
/// re-issued it
#[derive(Debug)]
pub struct Authed<T> {
    pub value: T,
    pub refreshed: Option<UpstreamSession>,
}

/// Finds and remembers where mgmtinterface-api is mounted. Shared by all requests.
#[derive(Default)]
pub struct MgmtApi {
    discovered: Mutex<Option<Discovered>>,
}

#[derive(Clone)]
struct Discovered {
    server: Url,
    base: Url,
}

impl MgmtApi {
    pub fn new() -> Self {
        Self::default()
    }

    /// The API below `server` (the configured `mgmt_api_url`), looked up on first use and again
    /// whenever the configured URL changes or a call suggests the API moved
    pub async fn connect(
        self: &Arc<Self>,
        server: &Url,
    ) -> Result<MgmtApiConnection, MgmtApiError> {
        let mut discovered = self.discovered.lock().await;
        let base = match discovered.as_ref() {
            Some(found) if &found.server == server => found.base.clone(),
            _ => {
                let base = discover(server).await?;
                *discovered = Some(Discovered {
                    server: server.clone(),
                    base: base.clone(),
                });
                base
            }
        };
        Ok(MgmtApiConnection {
            api: Arc::clone(self),
            base,
        })
    }

    async fn forget(&self, base: &Url) {
        let mut discovered = self.discovered.lock().await;
        if discovered.as_ref().is_some_and(|found| &found.base == base) {
            *discovered = None;
        }
    }
}

fn mount_point_url(server: &Url, mount_point: &str) -> Result<Url, MgmtApiError> {
    let joined = format!("{}{mount_point}", server.as_str().trim_end_matches('/'));
    Url::parse(&joined).map_err(|error| MgmtApiError::InvalidResponse(error.to_string()))
}

async fn discover(server: &Url) -> Result<Url, MgmtApiError> {
    let mut tried = Vec::new();
    let mut transport_errors = Vec::new();
    for mount_point in MOUNT_POINTS {
        let base = mount_point_url(server, mount_point)?;
        tried.push(format!("{base}/versions"));
        match http_client(&base, None)?
            .0
            .read_versions_mgmt_api_versions_get()
            .await
        {
            Ok(versions) if versions.supported.contains(&API_VERSION_NUMBER) => {
                info!("management API found at {base}");
                return Ok(base);
            }
            Ok(versions) => {
                warn!(
                    "the management API at {base} doesn't support version {API_VERSION} \
                     (supported: {:?}); this agent needs an update",
                    versions.supported
                );
            }
            // not mounted here, or something else answers here (the legacy API)
            Err(ApiOpError::Api(_)) => {}
            Err(ApiOpError::Transport(error)) => transport_errors.push(error.to_string()),
        }
    }
    // the server itself didn't answer, rather than answering without the API
    let error = if transport_errors.len() == MOUNT_POINTS.len() {
        MgmtApiError::Unreachable(transport_errors.join("; "))
    } else {
        MgmtApiError::NotFound {
            server: server.clone(),
            tried: tried.join(", "),
        }
    };
    warn!("{error}");
    Err(error)
}

/// A client for one call, with a fresh cookie jar holding `session`
fn http_client(
    base: &Url,
    session: Option<&UpstreamSession>,
) -> Result<(HttpClient, Arc<Jar>), MgmtApiError> {
    install_rustls_crypto_provider();
    let jar = Arc::new(Jar::default());
    if let Some(session) = session {
        for cookie in session.0.split("; ") {
            jar.add_cookie_str(&format!("{cookie}; Path=/"), base);
        }
    }
    let client = reqwest::Client::builder()
        .user_agent(format!("modama-agent/{}", modama_git_version()))
        .cookie_provider(Arc::clone(&jar))
        .build()
        .map_err(|error| MgmtApiError::Unreachable(error.to_string()))?;
    let client = HttpClient::with_client(client).with_base_url(base.as_str().trim_end_matches('/'));
    Ok((client, jar))
}

fn session_in(jar: &Jar, base: &Url) -> Option<UpstreamSession> {
    let cookies = jar.cookies(base)?;
    cookies
        .to_str()
        .ok()
        .map(|cookies| UpstreamSession(cookies.to_owned()))
}

/// The API's error body (`{code, message}`), or FastAPI's own for a missing route
#[derive(serde::Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

fn map_error<E: std::fmt::Debug>(error: ApiOpError<E>) -> MgmtApiError {
    let error = match error {
        ApiOpError::Transport(error) => return MgmtApiError::Unreachable(error.to_string()),
        ApiOpError::Api(error) => error,
    };
    let body = serde_json::from_str::<ErrorBody>(&error.body).ok();
    match (error.status, body) {
        (200..=299, _) => MgmtApiError::InvalidResponse(
            error
                .parse_error
                .unwrap_or_else(|| "unexpected response".to_owned()),
        ),
        (401, _) => MgmtApiError::Unauthorized,
        (403, Some(ErrorBody { code, message })) => MgmtApiError::Forbidden { code, message },
        (404, Some(ErrorBody { code, message })) => MgmtApiError::Missing { code, message },
        (404, None) => MgmtApiError::NoRoute,
        (status, Some(ErrorBody { message, .. })) => MgmtApiError::Backend { status, message },
        (status, None) => MgmtApiError::Backend {
            status,
            message: error.body,
        },
    }
}

/// mgmtinterface-api at a known mount point
pub struct MgmtApiConnection {
    api: Arc<MgmtApi>,
    base: Url,
}

impl MgmtApiConnection {
    async fn finish<T, E: std::fmt::Debug>(
        &self,
        result: Result<T, ApiOpError<E>>,
    ) -> Result<T, MgmtApiError> {
        let result = result.map_err(map_error);
        if let Err(error) = &result
            && error.invalidates_discovery()
        {
            self.api.forget(&self.base).await;
        }
        result
    }

    async fn authed<T, E: std::fmt::Debug>(
        &self,
        session: &UpstreamSession,
        jar: &Jar,
        result: Result<T, ApiOpError<E>>,
    ) -> Result<Authed<T>, MgmtApiError> {
        let value = self.finish(result).await?;
        let current = session_in(jar, &self.base);
        let refreshed = current.filter(|current| current != session);
        Ok(Authed { value, refreshed })
    }

    /// Open a session (a normal one: agents run on shared computers, so no "keep me logged in")
    pub async fn login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(String, UpstreamSession), MgmtApiError> {
        let (client, jar) = http_client(&self.base, None)?;
        let request = LoginRequest {
            username: username.to_owned(),
            password: password.to_owned(),
            persistent: None,
        };
        let info = self
            .finish(
                client
                    .login_mgmt_api_auth_login_post(API_VERSION, request)
                    .await,
            )
            .await?;
        let session = session_in(&jar, &self.base).ok_or_else(|| {
            MgmtApiError::InvalidResponse("the login answer carries no session cookie".to_owned())
        })?;
        Ok((info.username, session))
    }

    pub async fn logout(&self, session: &UpstreamSession) -> Result<(), MgmtApiError> {
        let (client, _jar) = http_client(&self.base, Some(session))?;
        self.finish(client.logout_mgmt_api_auth_logout_post(API_VERSION).await)
            .await
    }

    pub async fn projects(
        &self,
        session: &UpstreamSession,
    ) -> Result<Authed<ProjectList>, MgmtApiError> {
        let (client, jar) = http_client(&self.base, Some(session))?;
        let result = client
            .read_projects_mgmt_api_projects_get(API_VERSION)
            .await;
        self.authed(session, &jar, result).await
    }

    pub async fn project(
        &self,
        session: &UpstreamSession,
        project: &str,
    ) -> Result<Authed<ProjectDetails>, MgmtApiError> {
        let (client, jar) = http_client(&self.base, Some(session))?;
        let result = client
            .read_project_mgmt_api_projects_project_get(project, API_VERSION)
            .await;
        self.authed(session, &jar, result).await
    }

    pub async fn create_session(
        &self,
        session: &UpstreamSession,
        project: &str,
        name: &str,
    ) -> Result<Authed<()>, MgmtApiError> {
        let (client, jar) = http_client(&self.base, Some(session))?;
        let request = CreateSessionRequest {
            name: name.to_owned(),
        };
        let result = client
            .add_session_mgmt_api_projects_project_sessions_post(project, API_VERSION, request)
            .await
            .map(|_| ());
        self.authed(session, &jar, result).await
    }

    /// Open an ingest share bound to this agent's instrument; an open one for the same user,
    /// session and instrument is returned instead of a second one
    pub async fn open_ingest_share(
        &self,
        session: &UpstreamSession,
        project: &str,
        session_name: &str,
        resource: &str,
        agent_token: &str,
    ) -> Result<Authed<IngestShare>, MgmtApiError> {
        let (client, jar) = http_client(&self.base, Some(session))?;
        let request = CreateIngestShareRequest {
            project: project.to_owned(),
            session: session_name.to_owned(),
            resource: Some(Some(resource.to_owned())),
        };
        let result = client
            .add_ingest_share_mgmt_api_ingest_shares_post(Some(agent_token), API_VERSION, request)
            .await;
        self.authed(session, &jar, result).await
    }

    pub async fn close_ingest_share(
        &self,
        session: &UpstreamSession,
        share_id: &str,
    ) -> Result<Authed<()>, MgmtApiError> {
        let (client, jar) = http_client(&self.base, Some(session))?;
        let result = client
            .close_ingest_share_mgmt_api_ingest_shares_share_id_delete(share_id, API_VERSION)
            .await;
        self.authed(session, &jar, result).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mockito::{Matcher, Server, ServerGuard};
    use url::Url;

    use super::{MgmtApi, MgmtApiError, UpstreamSession};

    const VERSIONS: &str = r#"{"supported": [1], "latest": 1, "deprecated": []}"#;
    const SESSION: &str = "modama_session=abc";

    fn server_url(server: &ServerGuard) -> Url {
        Url::parse(&server.url()).unwrap()
    }

    async fn api_at_mgmt_api(server: &mut ServerGuard) -> Arc<MgmtApi> {
        server
            .mock("GET", "/api/versions")
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/mgmt-api/versions")
            .with_body(VERSIONS)
            .create_async()
            .await;
        Arc::new(MgmtApi::new())
    }

    #[tokio::test]
    async fn api_mount_point_wins_over_mgmt_api() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/versions")
            .with_body(VERSIONS)
            .create_async()
            .await;
        let mgmt_api = server
            .mock("GET", "/mgmt-api/versions")
            .expect(0)
            .create_async()
            .await;
        let projects = server
            .mock("GET", "/api/projects")
            .with_body(r#"{"projects": []}"#)
            .create_async()
            .await;

        let api = Arc::new(MgmtApi::new());
        let connection = api.connect(&server_url(&server)).await.unwrap();
        connection
            .projects(&UpstreamSession::from_token(SESSION))
            .await
            .unwrap();
        projects.assert_async().await;
        mgmt_api.assert_async().await;
    }

    #[tokio::test]
    async fn answers_that_are_not_a_suitable_versions_list_are_skipped() {
        let mut server = Server::new_async().await;
        // the legacy Flask API answers below /api/, but not with a versions list:
        server
            .mock("GET", "/api/versions")
            .with_body("<html>legacy</html>")
            .create_async()
            .await;
        // a newer API that dropped our version:
        server
            .mock("GET", "/mgmt-api/versions")
            .with_body(r#"{"supported": [2], "latest": 2, "deprecated": []}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/versions")
            .with_status(404)
            .create_async()
            .await;

        let api = Arc::new(MgmtApi::new());
        let error = api.connect(&server_url(&server)).await.err().unwrap();
        assert!(matches!(error, MgmtApiError::NotFound { .. }), "{error:?}");
    }

    #[tokio::test]
    async fn a_url_that_already_points_at_the_api_works() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/mgmt-api/api/versions")
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/mgmt-api/mgmt-api/versions")
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/mgmt-api/versions")
            .with_body(VERSIONS)
            .create_async()
            .await;
        let configured = Url::parse(&format!("{}/mgmt-api/", server.url())).unwrap();
        assert!(Arc::new(MgmtApi::new()).connect(&configured).await.is_ok());
    }

    #[tokio::test]
    async fn an_unreachable_server_is_reported_as_such() {
        // nothing listens on port 9 (discard) on a test machine
        let server = Url::parse("http://127.0.0.1:9").unwrap();
        let error = Arc::new(MgmtApi::new())
            .connect(&server)
            .await
            .err()
            .unwrap();
        assert!(matches!(error, MgmtApiError::Unreachable(_)), "{error:?}");
    }

    #[tokio::test]
    async fn login_opens_a_normal_session_and_returns_its_cookie() {
        let mut server = Server::new_async().await;
        let api = api_at_mgmt_api(&mut server).await;
        let login = server
            .mock("POST", "/mgmt-api/auth/login")
            .match_header("x-modama-api-version", "1")
            // no "persistent": agents run on shared computers
            .match_body(Matcher::Json(serde_json::json!({
                "username": "alice",
                "password": "secret",
            })))
            .with_header(
                "set-cookie",
                "modama_session=abc; HttpOnly; Path=/; SameSite=lax",
            )
            .with_body(r#"{"username": "alice", "persistent": false}"#)
            .create_async()
            .await;

        let connection = api.connect(&server_url(&server)).await.unwrap();
        let (username, session) = connection.login("alice", "secret").await.unwrap();
        login.assert_async().await;
        assert_eq!(username, "alice");
        assert_eq!(session.token(), SESSION);
    }

    #[tokio::test]
    async fn calls_send_the_session_and_return_a_reissued_one() {
        let mut server = Server::new_async().await;
        let api = api_at_mgmt_api(&mut server).await;
        server
            .mock("GET", "/mgmt-api/projects")
            .match_header("cookie", SESSION)
            .match_header("x-modama-api-version", "1")
            .with_body(r#"{"projects": []}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/mgmt-api/projects/p")
            .match_header("cookie", SESSION)
            .with_header(
                "set-cookie",
                "modama_session=def; HttpOnly; Path=/; SameSite=lax",
            )
            .with_body(
                r#"{"name": "p", "owner": "alice", "members": [], "ctime": 1.0, "mtime": 2.0,
                    "gid": 5001, "relpath": "adhoc/p", "usage_bytes": null, "sessions": []}"#,
            )
            .create_async()
            .await;

        let connection = api.connect(&server_url(&server)).await.unwrap();
        let session = UpstreamSession::from_token(SESSION);
        let unchanged = connection.projects(&session).await.unwrap();
        assert_eq!(unchanged.refreshed, None);
        let reissued = connection.project(&session, "p").await.unwrap();
        assert_eq!(
            reissued.refreshed,
            Some(UpstreamSession::from_token("modama_session=def"))
        );
    }

    #[tokio::test]
    async fn open_ingest_share_binds_the_share_to_the_instrument() {
        let mut server = Server::new_async().await;
        let api = api_at_mgmt_api(&mut server).await;
        let open = server
            .mock("POST", "/mgmt-api/ingest-shares")
            .match_header("x-agent-token", "the-token")
            .match_body(Matcher::Json(serde_json::json!({
                "project": "p", "session": "s", "resource": "holo",
            })))
            .with_status(201)
            .with_body(
                r#"{"share_id": "id", "share_name": "share-id", "project": "p",
                    "session": "s", "dest_path": "/data/adhoc/p/s", "created_timestamp": 0,
                    "username": "u", "password": "pw", "uid": 1, "gid": 1, "resource": "holo",
                    "created_by": "alice", "smb_host": "host", "workgroup": "W"}"#,
            )
            .create_async()
            .await;

        let connection = api.connect(&server_url(&server)).await.unwrap();
        let share = connection
            .open_ingest_share(
                &UpstreamSession::from_token(SESSION),
                "p",
                "s",
                "holo",
                "the-token",
            )
            .await
            .unwrap();
        open.assert_async().await;
        assert_eq!(share.value.share_id, "id");
        assert_eq!(share.value.created_by, Some(Some("alice".to_owned())));
    }

    #[tokio::test]
    async fn errors_are_mapped_by_status_and_code() {
        let mut server = Server::new_async().await;
        let api = api_at_mgmt_api(&mut server).await;
        server
            .mock("GET", "/mgmt-api/projects")
            .with_status(401)
            .with_body(r#"{"code": "not_authenticated", "message": "Please sign in."}"#)
            .create_async()
            .await;
        server
            .mock("DELETE", "/mgmt-api/ingest-shares/theirs")
            .with_status(403)
            .with_body(r#"{"code": "not_share_owner", "message": "Not your share."}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/mgmt-api/projects/nope")
            .with_status(404)
            .with_body(r#"{"code": "project_not_found", "message": "No such project."}"#)
            .create_async()
            .await;

        let connection = api.connect(&server_url(&server)).await.unwrap();
        let session = UpstreamSession::from_token(SESSION);
        assert!(matches!(
            connection.projects(&session).await,
            Err(MgmtApiError::Unauthorized)
        ));
        match connection.close_ingest_share(&session, "theirs").await {
            Err(MgmtApiError::Forbidden { code, message }) => {
                assert_eq!(
                    (code.as_str(), message.as_str()),
                    ("not_share_owner", "Not your share.")
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
        match connection.project(&session, "nope").await {
            Err(MgmtApiError::Missing { code, .. }) => assert_eq!(code, "project_not_found"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_missing_route_makes_the_next_connect_look_again() {
        let mut server = Server::new_async().await;
        let api = api_at_mgmt_api(&mut server).await;
        // FastAPI's own 404 for an unknown route, without the API's error body:
        server
            .mock("GET", "/mgmt-api/projects")
            .with_status(404)
            .with_body(r#"{"detail": "Not Found"}"#)
            .create_async()
            .await;

        let connection = api.connect(&server_url(&server)).await.unwrap();
        let error = connection
            .projects(&UpstreamSession::from_token(SESSION))
            .await
            .err()
            .unwrap();
        assert!(matches!(error, MgmtApiError::NoRoute), "{error:?}");

        // the API moved to /api in the meantime:
        server.reset();
        server
            .mock("GET", "/api/versions")
            .with_body(VERSIONS)
            .create_async()
            .await;
        let projects = server
            .mock("GET", "/api/projects")
            .with_body(r#"{"projects": []}"#)
            .create_async()
            .await;
        let connection = api.connect(&server_url(&server)).await.unwrap();
        connection
            .projects(&UpstreamSession::from_token(SESSION))
            .await
            .unwrap();
        projects.assert_async().await;
    }
}
