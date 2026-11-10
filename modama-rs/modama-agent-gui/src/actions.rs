use crate::state::{
    ActiveSession, AgentIdentity, ForegroundSession, OperationId, PendingSession, SignedIn,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryAction {
    Login,
    FetchProjects,
    FetchSessions { project: String },
    CreateIngest { project: String, session: String },
    CreateSession { project: String, session: String },
    CloseSession,
    StopAndLogOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskFailure {
    Stay { message: String, retry: RetryAction },
    ReturnToOverview { message: String, retry: RetryAction },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginDestination {
    Overview,
    SelectProject,
    /// Start a session: the overview asks first if a share is open, else project selection
    StartSession,
    ConfirmClose {
        share_id: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Connected {
        identity: AgentIdentity,
        foreground: ForegroundSession,
    },
    GoToLogin {
        destination: LoginDestination,
    },
    CreateSession,
    /// Start a session, and close `previous` once the new one has been created
    CreateSessionClosingPrevious {
        previous: ActiveSession,
    },
    LoginSuccess {
        signed_in: SignedIn,
        destination: LoginDestination,
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
    /// Release the previous user's drive that blocks the hand-off
    ForceRelease,
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
    /// The user asked to log out; the app reminds them of their ingest shares first
    LogoutRequested,
    LogoutSuccess,
    TaskFailed(TaskFailure),
}
