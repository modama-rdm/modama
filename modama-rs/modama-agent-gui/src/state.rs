pub struct ActiveSession {
}

pub enum AgentGuiState {
    Connecting,
    Overview { session: Option<ActiveSession>, },
    Login,
    SessionSelectProject,
    SelectCreateSession,
}

impl AgentGuiState {
    pub fn new() -> Self {
        todo!();
    }
}
