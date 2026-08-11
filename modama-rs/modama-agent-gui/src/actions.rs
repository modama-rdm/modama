use crate::state::{AgentIdentity, Credentials, ForegroundSession, OperationId, PendingSession};

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
    StartRequested {
        operation_id: OperationId,
        project: String,
        session: String,
    },
    IngestCreated {
        operation_id: OperationId,
        session: PendingSession,
    },
    StartFailed {
        operation_id: OperationId,
        failure: Option<TaskFailure>,
    },
    ForegroundChanged(ForegroundSession),
    GoToOverview,
    CloseRequested {
        operation_id: OperationId,
        share_id: String,
    },
    CloseSessionSuccess {
        operation_id: OperationId,
    },
    CloseFailed {
        operation_id: OperationId,
        failure: TaskFailure,
    },
    LogoutSuccess,
    TaskFailed(TaskFailure),
}
