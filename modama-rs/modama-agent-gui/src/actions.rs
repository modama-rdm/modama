use crate::state::{ActiveSession, AgentIdentity, Credentials};

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Connected { identity: AgentIdentity },
    GoToLogin,
    CreateSession,
    LoginSuccess { creds: Credentials },
    GoToSelectProject,
    SelectProject { project: String },
    SelectSession { session: ActiveSession },
    GoToOverview,
    CloseSessionSuccess,
    LogoutSuccess,
}
