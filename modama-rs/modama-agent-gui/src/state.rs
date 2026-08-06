#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSession {
    pub share_id: String,
    pub session: String,
    pub project: String,
    pub mount_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingSession {
    pub share_id: String,
    pub session: String,
    pub project: String,
}

impl PendingSession {
    pub fn matches(&self, share_id: &str) -> bool {
        self.share_id == share_id
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ForegroundSession {
    #[default]
    None,
    Mounting(PendingSession),
    Unmounting(ActiveSession),
    Mounted(ActiveSession),
}

impl ForegroundSession {
    pub fn share_id(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::Mounting(session) => Some(&session.share_id),
            Self::Unmounting(session) | Self::Mounted(session) => Some(&session.share_id),
        }
    }

    pub fn active_session(&self) -> Option<&ActiveSession> {
        match self {
            Self::Unmounting(session) | Self::Mounted(session) => Some(session),
            Self::None | Self::Mounting(_) => None,
        }
    }

    pub fn is_unmounting(&self, share_id: &str) -> bool {
        matches!(self, Self::Unmounting(session) if session.share_id == share_id)
    }
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

    /// Foreground ingest state reported by the local agent.
    pub foreground: ForegroundSession,

    /// A successfully created share that has not appeared in agent state yet.
    pub pending_start: Option<PendingSession>,

    /// A successfully deleted share that has not disappeared from agent state yet.
    pub pending_close: Option<String>,

    /// Agent mTLS identity, should be set once we are connected
    pub identity: Option<AgentIdentity>,
}

impl AgentAppState {
    pub fn new() -> Self {
        Self::default()
    }
}
