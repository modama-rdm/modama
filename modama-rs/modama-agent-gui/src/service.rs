use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::{
    api_client::{
        APIAgentIdentity, APICreateSessionRequest, APICurrentForegroundShare,
        APIHealthCheckResponse, APILoginRequest, AgentClient, generated::client::ApiOpError,
    },
    state::{
        ActiveSession, AgentIdentity, Credentials, ForegroundSession, PendingSession, SignedIn,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSummary {
    pub name: String,
    pub owner: String,
    pub members: Vec<String>,
    pub modified_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedAgent {
    pub identity: AgentIdentity,
    pub foreground: ForegroundSession,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ServiceError {
    #[error("authentication failed")]
    Unauthorized,
    #[error("object not found")]
    NotFound,
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("server error ({status}): {message}")]
    Server { status: u16, message: String },
}

pub trait AgentService: Send + Sync {
    /// The local agent's base URL, for code that runs next to the GUI (notebooks)
    fn agent_url(&self) -> &str {
        crate::config::DEFAULT_AGENT_URL
    }
    /// Show `path` in the file manager, in the background; failures are only logged
    fn open_folder(&self, _path: &str) {}
    fn connect(&self) -> Result<ConnectedAgent, ServiceError>;
    fn health(&self) -> Result<(), ServiceError>;
    fn foreground(&self) -> Result<ForegroundSession, ServiceError>;
    fn login(&self, credentials: &Credentials) -> Result<SignedIn, ServiceError>;
    /// Forget the session; the agent is told in the background
    fn logout(&self) {}
    /// Whether the session ended since the last call (an authenticated call was refused), so
    /// the user has to sign in again
    fn take_session_ended(&self) -> bool {
        false
    }
    fn list_projects(&self, signed_in: &SignedIn) -> Result<Vec<ProjectSummary>, ServiceError>;
    fn list_sessions(
        &self,
        signed_in: &SignedIn,
        project: &str,
    ) -> Result<Vec<String>, ServiceError>;
    fn create_session(
        &self,
        signed_in: &SignedIn,
        project: &str,
        session: &str,
    ) -> Result<(), ServiceError>;
    fn start_ingest(
        &self,
        signed_in: &SignedIn,
        project: &str,
        session: &str,
    ) -> Result<PendingSession, ServiceError>;
    fn close_session(
        &self,
        signed_in: &SignedIn,
        session: &ActiveSession,
    ) -> Result<(), ServiceError>;
}

pub struct HttpAgentService {
    runtime: tokio::runtime::Runtime,
    /// Holds the session cookie; replaced by a fresh one on login and logout
    client: RwLock<AgentClient>,
    session_ended: AtomicBool,
    agent_url: String,
}

impl HttpAgentService {
    pub fn new(agent_url: impl Into<String>) -> Result<Self, ServiceError> {
        let agent_url = agent_url.into();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| ServiceError::Transport(error.to_string()))?;
        Ok(Self {
            runtime,
            client: RwLock::new(AgentClient::new(agent_url.clone())),
            session_ended: AtomicBool::new(false),
            agent_url,
        })
    }

    pub fn agent_url(&self) -> &str {
        &self.agent_url
    }

    fn client(&self) -> AgentClient {
        match self.client.read() {
            Ok(client) => client.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Start over with an empty cookie jar, returning the client that held the session
    fn replace_client(&self) -> AgentClient {
        let fresh = AgentClient::new(self.agent_url.clone());
        match self.client.write() {
            Ok(mut client) => std::mem::replace(&mut *client, fresh),
            Err(poisoned) => std::mem::replace(&mut *poisoned.into_inner(), fresh),
        }
    }

    /// The result of an authenticated call; a 401 means the session ended
    fn authenticated<T, E: std::fmt::Debug>(
        &self,
        result: Result<T, ApiOpError<E>>,
    ) -> Result<T, ServiceError> {
        let result = result.map_err(map_api_error);
        if matches!(result, Err(ServiceError::Unauthorized)) {
            self.session_ended.store(true, Ordering::SeqCst);
        }
        result
    }

    fn foreground(&self) -> Result<ForegroundSession, ServiceError> {
        let response = self
            .runtime
            .block_on(self.client().get_foreground_share())
            .map_err(map_api_error)?;
        Ok(match response {
            APICurrentForegroundShare::APICurrentForegroundShareVariant(_) => {
                ForegroundSession::None
            }
            // Unavailable
            APICurrentForegroundShare::APICurrentForegroundShareVariant2(share) => {
                ForegroundSession::Mounting(
                    PendingSession {
                        share_id: share.share_id,
                        project: share.project,
                        session: share.session,
                        created_by: share.created_by.flatten(),
                    },
                    share.message,
                )
            }
            // Mounted
            APICurrentForegroundShare::APICurrentForegroundShareVariant3(share) => {
                ForegroundSession::Mounted(ActiveSession {
                    share_id: share.share_id,
                    project: share.project,
                    session: share.session,
                    mount_path: Some(share.mount_path),
                    created_by: share.created_by.flatten(),
                })
            }
        })
    }
}

impl From<modama_common::local_api_messages::APICurrentForegroundShare> for ForegroundSession {
    fn from(foreground: modama_common::local_api_messages::APICurrentForegroundShare) -> Self {
        use modama_common::local_api_messages::APICurrentForegroundShare as Foreground;
        match foreground {
            Foreground::None => ForegroundSession::None,
            Foreground::Unavailable {
                share_id,
                project,
                session,
                dest_path: _,
                message,
                created_by,
            } => ForegroundSession::Mounting(
                PendingSession {
                    share_id,
                    project,
                    session,
                    created_by,
                },
                message,
            ),
            Foreground::Mounted {
                share_id,
                project,
                session,
                dest_path: _,
                mount_path,
                created_by,
            } => ForegroundSession::Mounted(ActiveSession {
                share_id,
                project,
                session,
                mount_path: Some(mount_path),
                created_by,
            }),
        }
    }
}

fn map_api_error<E: std::fmt::Debug>(error: ApiOpError<E>) -> ServiceError {
    match error {
        ApiOpError::Transport(error) => ServiceError::Transport(error.to_string()),
        ApiOpError::Api(error) => match error.status {
            401 => ServiceError::Unauthorized,
            404 => ServiceError::NotFound,
            200..=299 => ServiceError::InvalidResponse(
                error
                    .parse_error
                    .unwrap_or_else(|| "unexpected success response".to_owned()),
            ),
            status => ServiceError::Server {
                status,
                message: error.body,
            },
        },
    }
}

impl AgentService for HttpAgentService {
    fn agent_url(&self) -> &str {
        &self.agent_url
    }

    fn open_folder(&self, path: &str) {
        use modama_session_helper::{
            mount_popup::FolderOpener as _, open_folder::SystemFolderOpener,
        };

        let path = std::path::PathBuf::from(path);
        let handle = self.runtime.handle().clone();
        // the opener's future isn't Send, so it runs on a blocking thread of its own
        self.runtime
            .spawn_blocking(move || handle.block_on(SystemFolderOpener.open(&path)));
    }

    fn connect(&self) -> Result<ConnectedAgent, ServiceError> {
        let identity = self
            .runtime
            .block_on(self.client().get_identity())
            .map_err(map_api_error)?;
        let identity = match identity {
            APIAgentIdentity::APIAgentIdentityVariant(identity) => AgentIdentity::Anonymous {
                hostname: identity.hostname,
            },
            APIAgentIdentity::APIAgentIdentityVariant2(identity) => AgentIdentity::AgentId {
                agent_name: identity.agent_name,
                resource_name: identity.resource_name,
            },
        };
        Ok(ConnectedAgent {
            identity,
            foreground: self.foreground()?,
        })
    }

    fn health(&self) -> Result<(), ServiceError> {
        match self
            .runtime
            .block_on(self.client().health())
            .map_err(map_api_error)?
        {
            APIHealthCheckResponse::APIHealthCheckResponseVariant(_) => Ok(()),
            APIHealthCheckResponse::APIHealthCheckResponseVariant2(_) => Err(
                ServiceError::InvalidResponse("agent reported that it is not available".to_owned()),
            ),
        }
    }

    fn foreground(&self) -> Result<ForegroundSession, ServiceError> {
        Self::foreground(self)
    }

    fn login(&self, credentials: &Credentials) -> Result<SignedIn, ServiceError> {
        // a new sign-in starts from an empty cookie jar, whoever was signed in before
        self.replace_client();
        self.session_ended.store(false, Ordering::SeqCst);
        let response = self
            .runtime
            .block_on(self.client().login(APILoginRequest {
                username: credentials.username.clone(),
                password: credentials.password.clone(),
            }))
            .map_err(map_api_error)?;
        Ok(SignedIn {
            username: response.username,
        })
    }

    fn logout(&self) {
        let previous = self.replace_client();
        self.runtime.spawn(async move {
            if let Err(error) = previous.logout().await {
                log::warn!("signing out at the agent failed: {error}");
            }
        });
    }

    fn take_session_ended(&self) -> bool {
        self.session_ended.swap(false, Ordering::SeqCst)
    }

    fn list_projects(&self, _signed_in: &SignedIn) -> Result<Vec<ProjectSummary>, ServiceError> {
        let response = self.authenticated(self.runtime.block_on(self.client().list_projects()))?;
        Ok(response
            .projects
            .into_iter()
            .map(|project| ProjectSummary {
                name: project.name,
                owner: project.owner,
                members: project.members,
                modified_at: project.modified_at,
            })
            .collect())
    }

    fn list_sessions(
        &self,
        _signed_in: &SignedIn,
        project: &str,
    ) -> Result<Vec<String>, ServiceError> {
        let response = self.authenticated(
            self.runtime
                .block_on(self.client().list_sessions_in_project(project)),
        )?;
        Ok(newest_first(
            response.sessions,
            response
                .session_summaries
                .into_iter()
                .flatten()
                .map(|summary| (summary.name, summary.modified_at)),
        ))
    }

    fn create_session(
        &self,
        _signed_in: &SignedIn,
        project: &str,
        session: &str,
    ) -> Result<(), ServiceError> {
        self.authenticated(
            self.runtime
                .block_on(self.client().create_session_in_project(
                    project,
                    APICreateSessionRequest {
                        session: session.to_owned(),
                    },
                )),
        )?;
        Ok(())
    }

    fn start_ingest(
        &self,
        _signed_in: &SignedIn,
        project: &str,
        session: &str,
    ) -> Result<PendingSession, ServiceError> {
        let response = self.authenticated(
            self.runtime
                .block_on(self.client().start_ingest(project, session)),
        )?;
        if response.project != project || response.session != session {
            return Err(ServiceError::InvalidResponse(
                "started ingest share does not match the requested project and session".to_owned(),
            ));
        }
        Ok(PendingSession {
            share_id: response.share_id,
            project: response.project,
            session: response.session,
            created_by: None,
        })
    }

    fn close_session(
        &self,
        _signed_in: &SignedIn,
        session: &ActiveSession,
    ) -> Result<(), ServiceError> {
        self.authenticated(
            self.runtime
                .block_on(self.client().delete_share(&session.share_id)),
        )?;
        Ok(())
    }
}

#[derive(Default)]
struct StubState {
    foreground: ForegroundSession,
    /// Folders the GUI asked to show in the file manager, oldest first
    opened_folders: Vec<String>,
    /// The ingest shares the GUI asked to stop, oldest first
    stopped_shares: Vec<String>,
}

pub struct StubAgentService {
    state: Mutex<StubState>,
    delay: Duration,
    session_ended: AtomicBool,
}

impl Default for StubAgentService {
    fn default() -> Self {
        Self::new()
    }
}

impl StubAgentService {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(StubState::default()),
            delay: if cfg!(any(test, feature = "visual-tests")) {
                Duration::ZERO
            } else {
                Duration::from_millis(500)
            },
            session_ended: AtomicBool::new(false),
        }
    }

    /// The folders the GUI asked to show in the file manager, oldest first
    pub fn opened_folders(&self) -> Vec<String> {
        match self.state.lock() {
            Ok(state) => state.opened_folders.clone(),
            Err(poisoned) => poisoned.into_inner().opened_folders.clone(),
        }
    }

    /// The ingest shares the GUI asked to stop, by share ID, oldest first
    pub fn stopped_shares(&self) -> Vec<String> {
        match self.state.lock() {
            Ok(state) => state.stopped_shares.clone(),
            Err(poisoned) => poisoned.into_inner().stopped_shares.clone(),
        }
    }

    /// Act as if the session ended, so the app's next look sends the user to the login page
    pub fn end_session(&self) {
        self.session_ended.store(true, Ordering::SeqCst);
    }

    pub fn shared() -> Arc<dyn AgentService> {
        Arc::new(Self::new())
    }

    fn delay(&self) {
        std::thread::sleep(self.delay);
    }
}

impl AgentService for StubAgentService {
    fn open_folder(&self, path: &str) {
        match self.state.lock() {
            Ok(mut state) => state.opened_folders.push(path.to_owned()),
            Err(poisoned) => poisoned.into_inner().opened_folders.push(path.to_owned()),
        }
    }

    fn connect(&self) -> Result<ConnectedAgent, ServiceError> {
        self.delay();
        Ok(ConnectedAgent {
            identity: AgentIdentity::AgentId {
                agent_name: "k2".to_owned(),
                resource_name: "Holo".to_owned(),
            },
            foreground: self
                .state
                .lock()
                .map_err(|error| ServiceError::Transport(error.to_string()))?
                .foreground
                .clone(),
        })
    }

    fn health(&self) -> Result<(), ServiceError> {
        Ok(())
    }

    fn foreground(&self) -> Result<ForegroundSession, ServiceError> {
        Ok(self
            .state
            .lock()
            .map_err(|error| ServiceError::Transport(error.to_string()))?
            .foreground
            .clone())
    }

    fn login(&self, credentials: &Credentials) -> Result<SignedIn, ServiceError> {
        self.delay();
        if credentials.username.trim() == "stupid" {
            Err(ServiceError::Unauthorized)
        } else {
            Ok(SignedIn {
                username: credentials.username.clone(),
            })
        }
    }

    fn take_session_ended(&self) -> bool {
        self.session_ended.swap(false, Ordering::SeqCst)
    }

    fn list_projects(&self, _signed_in: &SignedIn) -> Result<Vec<ProjectSummary>, ServiceError> {
        self.delay();
        Ok(crate::pages::select_project::representative_projects())
    }

    fn list_sessions(
        &self,
        _signed_in: &SignedIn,
        _project: &str,
    ) -> Result<Vec<String>, ServiceError> {
        self.delay();
        Ok(["26", "20", "18", "16"]
            .into_iter()
            .map(|day| format!("2026-06-{day}-holo-stacks"))
            .collect())
    }

    fn create_session(
        &self,
        _signed_in: &SignedIn,
        _project: &str,
        session: &str,
    ) -> Result<(), ServiceError> {
        self.delay();
        if session.to_lowercase().contains("forbidden") {
            Err(ServiceError::Server {
                status: 400,
                message: "Session name was rejected by the server.".to_owned(),
            })
        } else {
            Ok(())
        }
    }

    fn start_ingest(
        &self,
        _signed_in: &SignedIn,
        project: &str,
        session: &str,
    ) -> Result<PendingSession, ServiceError> {
        self.delay();
        let pending = PendingSession {
            share_id: format!("stub-{project}-{session}"),
            project: project.to_owned(),
            session: session.to_owned(),
            created_by: None,
        };
        let active = ActiveSession {
            share_id: pending.share_id.clone(),
            project: pending.project.clone(),
            session: pending.session.clone(),
            mount_path: Some(format!("/todo/{project}/{session}/")),
            created_by: None,
        };
        self.state
            .lock()
            .map_err(|error| ServiceError::Transport(error.to_string()))?
            .foreground = ForegroundSession::Mounted(active);
        Ok(pending)
    }

    fn close_session(
        &self,
        _signed_in: &SignedIn,
        session: &ActiveSession,
    ) -> Result<(), ServiceError> {
        self.delay();
        let mut state = self
            .state
            .lock()
            .map_err(|error| ServiceError::Transport(error.to_string()))?;
        if state.foreground.share_id() == Some(&session.share_id) {
            state.foreground = ForegroundSession::None;
        }
        state.stopped_shares.push(session.share_id.clone());
        Ok(())
    }
}

/// The session names, the last modified first. Sessions without a modification time (all of
/// them, from an agent that doesn't send any) keep their order, after the others.
fn newest_first(
    names: Vec<String>,
    modified_at: impl IntoIterator<Item = (String, i64)>,
) -> Vec<String> {
    let modified_at: std::collections::HashMap<String, i64> = modified_at.into_iter().collect();
    let mut names = names;
    names.sort_by_key(|name| std::cmp::Reverse(modified_at.get(name).copied()));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_are_listed_last_modified_first() {
        let names = ["a", "b", "c", "d"].map(String::from).to_vec();
        let modified_at = [("a", 10), ("b", 30), ("c", 20)].map(|(n, t)| (n.to_owned(), t));
        assert_eq!(newest_first(names, modified_at), ["b", "c", "a", "d"]);
    }

    #[test]
    fn sessions_keep_their_order_without_modification_times() {
        let names = ["b", "a", "c"].map(String::from).to_vec();
        assert_eq!(newest_first(names, []), ["b", "a", "c"]);
    }

    fn credentials(username: &str) -> Credentials {
        Credentials {
            username: username.to_owned(),
            password: "secret".to_owned(),
        }
    }

    #[test]
    fn stub_restores_started_state_and_closes_it() -> Result<(), ServiceError> {
        let service = StubAgentService::new();
        let signed_in = SignedIn {
            username: "user".to_owned(),
        };
        let pending = service.start_ingest(&signed_in, "project", "session")?;
        let ForegroundSession::Mounted(session) = service.connect()?.foreground else {
            panic!("stub session was not mounted");
        };
        assert_eq!(session.share_id, pending.share_id);
        service.close_session(&signed_in, &session)?;
        assert_eq!(service.connect()?.foreground, ForegroundSession::None);
        Ok(())
    }

    #[test]
    fn stub_rejects_known_login_and_session_fixtures() {
        let service = StubAgentService::new();
        assert_eq!(
            service.login(&credentials("stupid")),
            Err(ServiceError::Unauthorized)
        );
        assert!(matches!(
            service.create_session(
                &SignedIn {
                    username: "user".to_owned()
                },
                "project",
                "forbidden-session"
            ),
            Err(ServiceError::Server { status: 400, .. })
        ));
    }

    #[test]
    fn http_service_accepts_healthy_agent() -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/health/")
            .match_header("x-api-version", "1.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"type":"Ok"}"#)
            .create();

        HttpAgentService::new(server.url())?.health()?;
        mock.assert();
        Ok(())
    }

    #[test]
    fn http_service_rejects_unhealthy_success_response() -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/health/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"type":"NotAvailable"}"#)
            .create();

        assert!(matches!(
            HttpAgentService::new(server.url())?.health(),
            Err(ServiceError::InvalidResponse(_))
        ));
        mock.assert();
        Ok(())
    }

    #[test]
    fn http_service_maps_unavailable_and_malformed_health() -> Result<(), Box<dyn std::error::Error>>
    {
        for (status, body, expected_status) in [
            (503, r#"{"type":"NotAvailable"}"#, Some(503)),
            (200, "not json", None),
        ] {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("GET", "/api/health/")
                .with_status(status)
                .with_header("content-type", "application/json")
                .with_body(body)
                .create();

            let error = HttpAgentService::new(server.url())?.health().unwrap_err();
            if let Some(expected_status) = expected_status {
                assert!(matches!(
                    error,
                    ServiceError::Server {
                        status,
                        ..
                    } if status == expected_status
                ));
            } else {
                assert!(matches!(error, ServiceError::InvalidResponse(_)));
            }
            mock.assert();
        }
        Ok(())
    }

    #[test]
    fn http_service_maps_health_transport_error() -> Result<(), Box<dyn std::error::Error>> {
        assert!(matches!(
            HttpAgentService::new("not a URL")?.health(),
            Err(ServiceError::Transport(_))
        ));
        Ok(())
    }

    #[test]
    fn http_service_maps_unavailable_foreground_state() -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/shares/foreground/")
            .match_header("x-api-version", "1.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"type":"Unavailable","share_id":"share","project":"project","session":"session","dest_path":"/project/session","message":"no helper"}"#,
            )
            .create();

        let service = HttpAgentService::new(server.url())?;
        assert_eq!(
            service.foreground()?,
            ForegroundSession::Mounting(
                PendingSession {
                    share_id: "share".to_owned(),
                    project: "project".to_owned(),
                    session: "session".to_owned(),
                    created_by: None,
                },
                "no helper".to_owned()
            )
        );
        mock.assert();
        Ok(())
    }

    #[test]
    fn http_service_maps_mounted_foreground_state() -> Result<(), Box<dyn std::error::Error>> {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/shares/foreground/")
            .match_header("x-api-version", "1.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"type":"Mounted","share_id":"share","project":"project","session":"session","dest_path":"/project/session","mount_path":"/mnt/session"}"#,
            )
            .create();

        let service = HttpAgentService::new(server.url())?;
        assert_eq!(
            service.foreground()?,
            ForegroundSession::Mounted(ActiveSession {
                share_id: "share".to_owned(),
                project: "project".to_owned(),
                session: "session".to_owned(),
                mount_path: Some("/mnt/session".to_owned()),
                created_by: None,
            })
        );
        mock.assert();
        Ok(())
    }
}
