#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSession {
    pub session: String,
    pub project: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// App state - general state of the whole app,
/// such as credentials and the active ingest session.
#[derive(Clone, Debug, Default)]
pub struct AgentAppState {
    /// if None, the user is logged out
    pub creds: Option<Credentials>,

    /// currently active ingest session, None if
    /// there isn't one (or we don't know about it)
    pub session: Option<ActiveSession>,
}

impl AgentAppState {
    pub fn new() -> Self {
        Self::default()
    }
}
