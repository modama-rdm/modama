#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSession {
    pub share_id: String,
    pub session: String,
    pub project: String,
    pub mount_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentIdentity {
    Anonymous {
        hostname: String,
    },
    AgentId {
        /// A user-readable short description of the data producer, like "DLD"
        agent_name: String,

        /// A short name of the resouce this agent belongs to, for example "Holo"
        resource_name: String,
    },
}

impl AgentIdentity {
    pub fn format(&self) -> String {
        match self {
            AgentIdentity::Anonymous { hostname } => format!("modama-agent @ {hostname}"),
            AgentIdentity::AgentId {
                agent_name,
                resource_name,
            } => format!("modama-agent {agent_name} on {resource_name}"),
        }
    }
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

    /// Agent mTLS identity, should be set once we are connected
    pub identity: Option<AgentIdentity>,
}

impl AgentAppState {
    pub fn new() -> Self {
        Self::default()
    }
}
