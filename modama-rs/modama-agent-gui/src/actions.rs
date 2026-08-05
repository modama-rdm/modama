use crate::state::{ActiveSession, AgentIdentity, Credentials};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryAction {
    Reconnect,
    Login,
    FetchProjects,
    FetchSessions { project: String },
    CreateIngest { project: String, session: String },
    CreateSession { project: String, session: String },
    CloseSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskFailure {
    Stay { message: String, retry: RetryAction },
    ReturnToOverview { message: String, retry: RetryAction },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Connected {
        identity: AgentIdentity,
        session: Option<ActiveSession>,
    },
    GoToLogin,
    CreateSession,
    LoginSuccess {
        creds: Credentials,
    },
    GoToSelectProject,
    SelectProject {
        project: String,
    },
    SelectSession {
        session: ActiveSession,
    },
    GoToOverview,
    CloseSessionSuccess,
    LogoutSuccess,
    TaskFailed(TaskFailure),
}
