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

    session_operations: SessionOperations,

    /// Agent mTLS identity, should be set once we are connected
    pub identity: Option<AgentIdentity>,
}

impl AgentAppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_start(&mut self, operation_id: OperationId, project: String, session: String) {
        self.session_operations.start = Some(StartOperation {
            operation_id,
            project,
            session,
            response: None,
            observed_share_ids: HashSet::new(),
        });
    }

    /// Record a successful start response. Returns false for a stale completion.
    pub fn finish_start(&mut self, operation_id: OperationId, response: PendingSession) -> bool {
        let Some(operation) = self.session_operations.start.as_mut() else {
            return false;
        };
        if operation.operation_id != operation_id {
            return false;
        }

        if operation.observed_share_ids.contains(&response.share_id) {
            self.session_operations.start = None;
        } else {
            operation.response = Some(response);
        }
        true
    }

    /// Retire a failed start. Returns false for a stale completion.
    pub fn fail_start(&mut self, operation_id: OperationId) -> bool {
        if self
            .session_operations
            .start
            .as_ref()
            .is_none_or(|operation| operation.operation_id != operation_id)
        {
            return false;
        }
        self.session_operations.start = None;
        true
    }

    pub fn begin_close(&mut self, operation_id: OperationId, share_id: String) {
        self.session_operations.close = Some(CloseOperation {
            operation_id,
            share_id,
            observed: false,
            response_received: false,
        });
    }

    /// Retire a successful close. Returns false for a stale completion.
    pub fn finish_close(&mut self, operation_id: OperationId) -> bool {
        if self
            .session_operations
            .close
            .as_ref()
            .is_none_or(|operation| operation.operation_id != operation_id)
        {
            return false;
        }
        if self
            .session_operations
            .close
            .as_ref()
            .is_some_and(|operation| operation.observed)
        {
            self.session_operations.close = None;
        } else if let Some(operation) = self.session_operations.close.as_mut() {
            operation.response_received = true;
        }
        true
    }

    /// Retire a failed close and report whether the failure is still relevant.
    pub fn fail_close(&mut self, operation_id: OperationId) -> Option<bool> {
        let operation = self.session_operations.close.as_ref()?;
        if operation.operation_id != operation_id {
            return None;
        }
        let should_report = !operation.observed;
        self.session_operations.close = None;
        Some(should_report)
    }

    pub fn observe_foreground(&mut self, foreground: ForegroundSession) {
        if let Some(operation) = self.session_operations.start.as_mut() {
            if let Some(share_id) = foreground.share_id() {
                operation.observed_share_ids.insert(share_id.to_owned());
            }
            if operation
                .response
                .as_ref()
                .is_some_and(|pending| foreground.share_id() == Some(pending.share_id.as_str()))
            {
                self.session_operations.start = None;
            }
        }

        if let Some(operation) = self.session_operations.close.as_mut() {
            if foreground.is_unmounting(&operation.share_id)
                || foreground
                    .share_id()
                    .is_none_or(|current| current != operation.share_id)
            {
                if operation.response_received {
                    self.session_operations.close = None;
                } else {
                    operation.observed = true;
                }
            }
        }

        self.foreground = foreground;
    }

    pub fn pending_start(&self) -> Option<&PendingSession> {
        self.session_operations
            .start
            .as_ref()
            .and_then(|operation| operation.response.as_ref())
    }

    pub fn pending_close(&self) -> Option<&str> {
        self.session_operations
            .close
            .as_ref()
            .filter(|operation| !operation.observed)
            .map(|operation| operation.share_id.as_str())
    }
}

#[derive(Clone, Debug, Default)]
struct SessionOperations {
    start: Option<StartOperation>,
    close: Option<CloseOperation>,
}

#[derive(Clone, Debug)]
struct StartOperation {
    operation_id: OperationId,
    #[allow(dead_code)]
    project: String,
    #[allow(dead_code)]
    session: String,
    response: Option<PendingSession>,
    observed_share_ids: HashSet<String>,
}

#[derive(Clone, Debug)]
struct CloseOperation {
    operation_id: OperationId,
    share_id: String,
    observed: bool,
    response_received: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(share_id: &str) -> PendingSession {
        PendingSession {
            share_id: share_id.to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
        }
    }

    fn mounted(share_id: &str) -> ForegroundSession {
        ForegroundSession::Mounted(ActiveSession {
            share_id: share_id.to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            mount_path: None,
        })
    }

    #[test]
    fn start_completion_after_matching_observation_does_not_restore_pending_state() {
        let mut state = AgentAppState::new();
        let operation_id = OperationId::next();
        state.begin_start(operation_id, "project".to_owned(), "session".to_owned());
        state.observe_foreground(mounted("share"));
        state.observe_foreground(ForegroundSession::None);

        assert!(state.finish_start(operation_id, pending("share")));
        assert!(state.pending_start().is_none());
    }

    #[test]
    fn unrelated_observation_does_not_complete_a_start() {
        let mut state = AgentAppState::new();
        let operation_id = OperationId::next();
        state.begin_start(operation_id, "project".to_owned(), "session".to_owned());
        state.observe_foreground(mounted("other"));

        assert!(state.finish_start(operation_id, pending("share")));
        assert_eq!(
            state
                .pending_start()
                .map(|pending| pending.share_id.as_str()),
            Some("share")
        );
    }

    #[test]
    fn matching_observation_completes_a_start_after_the_response() {
        let mut state = AgentAppState::new();
        let operation_id = OperationId::next();
        state.begin_start(operation_id, "project".to_owned(), "session".to_owned());
        assert!(state.finish_start(operation_id, pending("share")));

        state.observe_foreground(mounted("share"));

        assert!(state.pending_start().is_none());
    }

    #[test]
    fn observed_close_suppresses_a_late_failure() {
        let mut state = AgentAppState::new();
        let operation_id = OperationId::next();
        state.begin_close(operation_id, "share".to_owned());
        state.observe_foreground(ForegroundSession::None);

        assert_eq!(state.fail_close(operation_id), Some(false));
        assert!(state.pending_close().is_none());
    }

    #[test]
    fn observed_close_is_not_restored_by_late_success() {
        let mut state = AgentAppState::new();
        let operation_id = OperationId::next();
        state.begin_close(operation_id, "share".to_owned());
        state.observe_foreground(ForegroundSession::None);

        assert!(state.finish_close(operation_id));
        assert!(state.pending_close().is_none());
        assert!(state.session_operations.close.is_none());
    }
}
use std::{
    collections::HashSet,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationId(u64);

impl OperationId {
    pub fn next() -> Self {
        Self(NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed))
    }
}
