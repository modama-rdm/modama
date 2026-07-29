use crate::state::{ActiveSession, Credentials};

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    GoToLogin,
    CreateSession,
    LoginSuccess { creds: Credentials },
    GoToSelectProject,
    SelectProject { project: String },
    SelectSession { session: ActiveSession },
    GoToOverview,
}

impl Action {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}
