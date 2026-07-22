#[derive(Clone, Debug)]
pub struct ActiveSession {
    pub session: String,
    pub project: String,
}

// TODO: update to use cookie or token or whatever instead
#[derive(Clone, Debug)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// App state - general state of the whole app,
/// especially anything that is independent of the `AgentGuiState`
/// can become part of the app state directly.
#[derive(Clone, Debug)]
pub struct AgentAppState {
    pub gui_state: AgentGuiState,

    /// if None, the user is logged out
    pub creds: Option<Credentials>,

    /// currently active ingest session, None if
    /// there isn't one (or we don't know about it)
    pub session: Option<ActiveSession>,
}

// FIXME: there are some dependencies - when the GUI is on
// the Overview page, it should have ActiveSessions

impl AgentAppState {
    pub fn new() -> Self {
        Self {
            gui_state: AgentGuiState::new(),
            creds: None,
            session: None,
        }
    }
}


// TODO: background jobs, communication, spinning, timeouts, ...
// how can we make this as straight-forward as possible? maybe we
// store a lil thread join handle here that represents the
// background work, and poll that one until we have the data to go
// to the next step in the process?
// right now, the mutable state management is a bit awkwards - maybe 
// we should use an elm/redux/... like state management pattern
// instead of mutating self? needs to be explored!!

#[derive(Clone, Debug)]
pub enum AgentGuiState {
    Connecting,
    Overview {},
    /// Login UI state - username/password are those currenctly entered in the text input elements
    Login {
        username: String,
        password: String,
        waiting_for_next: bool,  // true if we spin a lil and wait for a response from the backend.
    },
    SessionSelectProject {
        selected_project: Option<String>,
    },
    SelectCreateSession {
        project: String,
        selected_session: Option<String>,
        new_session_input: String,
    },
}

impl AgentGuiState {
    pub fn new() -> Self {
        Self::Connecting
    }
}
