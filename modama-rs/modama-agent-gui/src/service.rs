use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{
    api_client::{
        generated::client::ApiOpError, APIAgentIdentity, APICreateSessionRequest,
        APICurrentForegroundShare, AgentClient,
    },
    state::{ActiveSession, AgentIdentity, Credentials},
};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const POLL_TIMEOUT: Duration = Duration::from_secs(30);

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
    pub session: Option<ActiveSession>,
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
    fn connect(&self) -> Result<ConnectedAgent, ServiceError>;
    fn login(&self, credentials: &Credentials) -> Result<(), ServiceError>;
    fn list_projects(&self, credentials: &Credentials)
        -> Result<Vec<ProjectSummary>, ServiceError>;
    fn list_sessions(
        &self,
        credentials: &Credentials,
        project: &str,
    ) -> Result<Vec<String>, ServiceError>;
    fn create_session(
        &self,
        credentials: &Credentials,
        project: &str,
        session: &str,
    ) -> Result<(), ServiceError>;
    fn start_ingest(
        &self,
        credentials: &Credentials,
        project: &str,
        session: &str,
    ) -> Result<ActiveSession, ServiceError>;
    fn close_session(
        &self,
        credentials: &Credentials,
        session: &ActiveSession,
    ) -> Result<(), ServiceError>;
}

pub struct HttpAgentService {
    runtime: tokio::runtime::Runtime,
    client: AgentClient,
}

impl HttpAgentService {
    pub fn new(agent_url: impl Into<String>) -> Result<Self, ServiceError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| ServiceError::Transport(error.to_string()))?;
        Ok(Self {
            runtime,
            client: AgentClient::new(agent_url),
        })
    }

    fn authenticated_client(&self, credentials: &Credentials) -> AgentClient {
        self.client
            .with_basic_auth(&credentials.username, &credentials.password)
    }

    fn foreground(&self) -> Result<Option<ActiveSession>, ServiceError> {
        let response = self
            .runtime
            .block_on(self.client.get_foreground_share())
            .map_err(map_api_error)?;
        Ok(match response {
            APICurrentForegroundShare::APICurrentForegroundShareVariant(_) => None,
            APICurrentForegroundShare::APICurrentForegroundShareVariant2(share) => {
                Some(ActiveSession {
                    share_id: share.share_id,
                    project: share.project,
                    session: share.session,
                    mount_path: Some(share.mount_path),
                })
            }
        })
    }

    fn wait_for_share(
        &self,
        share_id: &str,
        present: bool,
    ) -> Result<Option<ActiveSession>, ServiceError> {
        let start = Instant::now();
        loop {
            let foreground = self.foreground()?;
            if foreground
                .as_ref()
                .is_some_and(|share| share.share_id == share_id)
                == present
            {
                return Ok(foreground);
            }
            if start.elapsed() >= POLL_TIMEOUT {
                return Err(ServiceError::Transport(format!(
                    "timed out waiting for share {share_id} to {}",
                    if present { "mount" } else { "close" }
                )));
            }
            std::thread::sleep(POLL_INTERVAL);
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
    fn connect(&self) -> Result<ConnectedAgent, ServiceError> {
        let identity = self
            .runtime
            .block_on(self.client.get_identity())
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
            session: self.foreground()?,
        })
    }

    fn login(&self, credentials: &Credentials) -> Result<(), ServiceError> {
        self.runtime
            .block_on(self.authenticated_client(credentials).login())
            .map(|_| ())
            .map_err(map_api_error)
    }

    fn list_projects(
        &self,
        credentials: &Credentials,
    ) -> Result<Vec<ProjectSummary>, ServiceError> {
        self.runtime
            .block_on(self.authenticated_client(credentials).list_projects())
            .map(|response| {
                response
                    .projects
                    .into_iter()
                    .map(|project| ProjectSummary {
                        name: project.name,
                        owner: project.owner,
                        members: project.members,
                        modified_at: project.modified_at,
                    })
                    .collect()
            })
            .map_err(map_api_error)
    }

    fn list_sessions(
        &self,
        credentials: &Credentials,
        project: &str,
    ) -> Result<Vec<String>, ServiceError> {
        self.runtime
            .block_on(
                self.authenticated_client(credentials)
                    .list_sessions_in_project(project),
            )
            .map(|response| response.sessions)
            .map_err(map_api_error)
    }

    fn create_session(
        &self,
        credentials: &Credentials,
        project: &str,
        session: &str,
    ) -> Result<(), ServiceError> {
        self.runtime
            .block_on(
                self.authenticated_client(credentials)
                    .create_session_in_project(
                        project,
                        APICreateSessionRequest {
                            session: session.to_owned(),
                        },
                    ),
            )
            .map(|_| ())
            .map_err(map_api_error)
    }

    fn start_ingest(
        &self,
        credentials: &Credentials,
        project: &str,
        session: &str,
    ) -> Result<ActiveSession, ServiceError> {
        let response = self
            .runtime
            .block_on(
                self.authenticated_client(credentials)
                    .start_ingest(project, session),
            )
            .map_err(map_api_error)?;
        if response.project != project || response.session != session {
            return Err(ServiceError::InvalidResponse(
                "started share does not match the requested project and session".to_owned(),
            ));
        }
        self.wait_for_share(&response.share_id, true)?
            .ok_or_else(|| ServiceError::InvalidResponse("mounted share disappeared".to_owned()))
    }

    fn close_session(
        &self,
        credentials: &Credentials,
        session: &ActiveSession,
    ) -> Result<(), ServiceError> {
        self.runtime
            .block_on(
                self.authenticated_client(credentials)
                    .delete_share(&session.share_id),
            )
            .map_err(map_api_error)?;
        self.wait_for_share(&session.share_id, false)?;
        Ok(())
    }
}

#[derive(Default)]
struct StubState {
    foreground: Option<ActiveSession>,
}

pub struct StubAgentService {
    state: Mutex<StubState>,
    delay: Duration,
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
        }
    }

    pub fn shared() -> Arc<dyn AgentService> {
        Arc::new(Self::new())
    }

    fn delay(&self) {
        std::thread::sleep(self.delay);
    }
}

impl AgentService for StubAgentService {
    fn connect(&self) -> Result<ConnectedAgent, ServiceError> {
        self.delay();
        Ok(ConnectedAgent {
            identity: AgentIdentity::AgentId {
                agent_name: "k2".to_owned(),
                resource_name: "Holo".to_owned(),
            },
            session: self
                .state
                .lock()
                .map_err(|error| ServiceError::Transport(error.to_string()))?
                .foreground
                .clone(),
        })
    }

    fn login(&self, credentials: &Credentials) -> Result<(), ServiceError> {
        self.delay();
        if credentials.username.trim() == "stupid" {
            Err(ServiceError::Unauthorized)
        } else {
            Ok(())
        }
    }

    fn list_projects(
        &self,
        _credentials: &Credentials,
    ) -> Result<Vec<ProjectSummary>, ServiceError> {
        self.delay();
        Ok(crate::pages::select_project::representative_projects())
    }

    fn list_sessions(
        &self,
        _credentials: &Credentials,
        _project: &str,
    ) -> Result<Vec<String>, ServiceError> {
        self.delay();
        Ok(["16", "18", "20", "26"]
            .into_iter()
            .map(|day| format!("2026-06-{day}-holo-stacks"))
            .collect())
    }

    fn create_session(
        &self,
        _credentials: &Credentials,
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
        _credentials: &Credentials,
        project: &str,
        session: &str,
    ) -> Result<ActiveSession, ServiceError> {
        self.delay();
        let active = ActiveSession {
            share_id: format!("stub-{project}-{session}"),
            project: project.to_owned(),
            session: session.to_owned(),
            mount_path: Some(format!("/todo/{project}/{session}/")),
        };
        self.state
            .lock()
            .map_err(|error| ServiceError::Transport(error.to_string()))?
            .foreground = Some(active.clone());
        Ok(active)
    }

    fn close_session(
        &self,
        _credentials: &Credentials,
        session: &ActiveSession,
    ) -> Result<(), ServiceError> {
        self.delay();
        let mut state = self
            .state
            .lock()
            .map_err(|error| ServiceError::Transport(error.to_string()))?;
        if state
            .foreground
            .as_ref()
            .is_some_and(|active| active.share_id == session.share_id)
        {
            state.foreground = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials(username: &str) -> Credentials {
        Credentials {
            username: username.to_owned(),
            password: "secret".to_owned(),
        }
    }

    #[test]
    fn stub_restores_started_state_and_closes_it() -> Result<(), ServiceError> {
        let service = StubAgentService::new();
        let credentials = credentials("user");
        let session = service.start_ingest(&credentials, "project", "session")?;
        assert_eq!(service.connect()?.session, Some(session.clone()));
        service.close_session(&credentials, &session)?;
        assert!(service.connect()?.session.is_none());
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
            service.create_session(&credentials("user"), "project", "forbidden-session"),
            Err(ServiceError::Server { status: 400, .. })
        ));
    }
}
