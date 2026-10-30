use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    api_client::{
        APIAgentIdentity, APICreateSessionRequest, APICurrentForegroundShare,
        APIHealthCheckResponse, AgentClient, generated::client::ApiOpError,
    },
    state::{ActiveSession, AgentIdentity, Credentials, ForegroundSession, PendingSession},
};

pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(500);

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
    fn connect(&self) -> Result<ConnectedAgent, ServiceError>;
    fn health(&self) -> Result<(), ServiceError>;
    fn foreground(&self) -> Result<ForegroundSession, ServiceError>;
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
    ) -> Result<PendingSession, ServiceError>;
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

    fn foreground(&self) -> Result<ForegroundSession, ServiceError> {
        let response = self
            .runtime
            .block_on(self.client.get_foreground_share())
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
                })
            }
        })
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
            foreground: self.foreground()?,
        })
    }

    fn health(&self) -> Result<(), ServiceError> {
        match self
            .runtime
            .block_on(self.client.health())
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
    ) -> Result<PendingSession, ServiceError> {
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
        Ok(PendingSession {
            share_id: response.share_id,
            project: response.project,
            session: response.session,
        })
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
        Ok(())
    }
}

#[derive(Default)]
struct StubState {
    foreground: ForegroundSession,
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
    ) -> Result<PendingSession, ServiceError> {
        self.delay();
        let pending = PendingSession {
            share_id: format!("stub-{project}-{session}"),
            project: project.to_owned(),
            session: session.to_owned(),
        };
        let active = ActiveSession {
            share_id: pending.share_id.clone(),
            project: pending.project.clone(),
            session: pending.session.clone(),
            mount_path: Some(format!("/todo/{project}/{session}/")),
        };
        self.state
            .lock()
            .map_err(|error| ServiceError::Transport(error.to_string()))?
            .foreground = ForegroundSession::Mounted(active);
        Ok(pending)
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
        if state.foreground.share_id() == Some(&session.share_id) {
            state.foreground = ForegroundSession::None;
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
        let pending = service.start_ingest(&credentials, "project", "session")?;
        let ForegroundSession::Mounted(session) = service.connect()?.foreground else {
            panic!("stub session was not mounted");
        };
        assert_eq!(session.share_id, pending.share_id);
        service.close_session(&credentials, &session)?;
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
            service.create_session(&credentials("user"), "project", "forbidden-session"),
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
            })
        );
        mock.assert();
        Ok(())
    }
}
