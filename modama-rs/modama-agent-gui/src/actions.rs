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
    LogoutSuccess,
}

impl Action {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}
