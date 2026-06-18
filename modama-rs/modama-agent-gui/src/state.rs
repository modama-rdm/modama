#[derive(Clone, Debug)]
pub struct ActiveSession {
    pub session: String,
    pub project: String,
}

#[derive(Clone, Debug)]
pub enum AgentGuiState {
    Connecting,
    Overview {
        session: Option<ActiveSession>,
    },
    Login {
        username: String,
        password: String,
    },
    SessionSelectProject {
        selected_project: Option<String>,
    },
    SelectCreateSession {
        project: String,
        selected_session: Option<String>,
    },
}

// TODO:
// separate UI state from app state?
// I think there are parts of the state that are orthogonal
// to the currently displayed UI, for example
// if the user is currently logged in, there will be
// a username/password combination that is different from
// the values stored for keeping the login form state; and in
// the future these will be replaced with a session cookie.
// also things like a cache of all projects the user has access
// to...

impl AgentGuiState {
    pub fn new() -> Self {
        Self::Connecting
    }
}
