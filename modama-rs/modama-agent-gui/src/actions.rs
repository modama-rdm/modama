use crate::state::{AgentIdentity, Credentials, ForegroundSession, PendingSession};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryAction {
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
        foreground: ForegroundSession,
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
    IngestCreated {
        session: PendingSession,
    },
    ForegroundChanged(ForegroundSession),
    GoToOverview,
    CloseSessionSuccess {
        share_id: String,
    },
    LogoutSuccess,
    TaskFailed(TaskFailure),
}
