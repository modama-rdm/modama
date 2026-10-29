use std::{collections::HashMap, path::PathBuf, sync::Arc};

use log::{info, warn};
use modama_common::{agent::AgentId, internal_api_types::{MountedShare, SessionId}, messages::Share};
use tokio::sync::{Mutex, broadcast, mpsc};

use crate::{
    config::AgentConfig,
    events_api::{SessionBeginEnd, },
};

#[derive(Debug, Default)]
pub enum ConnectionState {
    #[default]
    Connecting,
    Authenticating,
    Connected,
}

#[derive(Debug, Clone)]
pub struct ShareState<T>
where
    T: PartialEq + Clone,
{
    /// The current foreground share, if any.
    pub foreground_share: Option<T>,

    /// All shares, including the foreground share
    pub shares: Vec<T>,
}

impl<T> ShareState<T>
where
    T: PartialEq + Clone + std::fmt::Debug,
{
    #[deprecated(note = "replace whole state instead")]
    pub fn set_foreground_share(&mut self, share: Option<T>) {
        self.foreground_share = share;
        self.ensure_consistent();
    }

    pub fn ensure_consistent(&mut self) {
        // ensure the foreground share is part of the list of all shares:
        if let Some(share) = &self.foreground_share
            && !self.shares.contains(share)
        {
            warn!(
                "correcting inconsistent state: foreground_share {:?} was not part of the share list",
                share
            );
            self.shares.push(share.clone());
        }
    }

    pub fn replace_state(&mut self, shares: Vec<T>, foreground_share: Option<T>) {
        self.shares = shares;
        self.foreground_share = foreground_share;
        self.ensure_consistent();
    }

    #[deprecated(note = "replace whole state instead")]
    pub fn set_shares(&mut self, shares: Vec<T>) {
        self.shares = shares;
        self.ensure_consistent();
    }
}

impl<T> Default for ShareState<T>
where
    T: PartialEq + Clone,
{
    fn default() -> Self {
        Self {
            foreground_share: None,
            shares: Vec::new(),
        }
    }
}

/// A mount joined with the `Share` it belongs to. Agent-internal, not part of the
/// wire format: helpers only report share names and paths (`MountedShare`), the
/// agent will later fill this by joining those reports with the desired shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMount {
    pub share: Share,

    /// The mount path; a drive letter on Windows, or a path on Linux
    pub mount_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub connected_agents: Vec<AgentId>,
    pub desired_shares: ShareState<Share>,
}

impl RemoteInfo {
    pub fn new() -> Self {
        Self {
            connected_agents: Default::default(),
            desired_shares: Default::default(),
        }
    }
}

impl Default for RemoteInfo {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum StateUpdateToSession {
    DesiredState {
        /// The current foreground share
        foreground_share_id: Option<String>,

        /// All shares that should be mounted
        shares: Vec<Share>,
    },
    MountUpdate {
        session_id: SessionId,
        /// In case the session helper disconnects, this is not an empty Vec
        /// but None! Meaning the GUI can distinguish these two cases if needed.
        shares: Option<Vec<MountedShare>>,
    },
}

#[derive(Debug)]
pub struct AgentState {
    pub connection: ConnectionState,

    /// The currently mounted shares, joined with their `Share`. Nothing fills this yet;
    /// see `ResolvedMount`.
    pub current_shares: ShareState<ResolvedMount>,

    /// Per-session list of currently mounted shares
    pub mounted_shares: HashMap<SessionId, Vec<MountedShare>>,

    /// Counter for active session helper connections
    /// (to prevent cleanup while there's still an active connection)
    pub session_connections: HashMap<SessionId, u64>,

    pub session_channel_sender: mpsc::UnboundedSender<SessionBeginEnd>,

    /// Info we received over the event channel; None if we haven't received any yet
    pub remote_info: Option<RemoteInfo>,

    /// Runtime configuration
    pub config: AgentConfig,

    /// Broadcast channel to update GUI and session helpers
    pub desired_state_channel: broadcast::Sender<StateUpdateToSession>,
}

/// Cleanup task: when connections for a session_id drop to zero, clear the session state
async fn session_bookkeeper(
    agent_state: Arc<Mutex<AgentState>>,
    mut channel: mpsc::UnboundedReceiver<SessionBeginEnd>,
) {
    loop {
        match channel.recv().await {
            Some(SessionBeginEnd::Begin { session_id }) => {
                let mut state = agent_state.lock().await;
                state
                    .session_connections
                    .entry(session_id.clone())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
            }
            Some(SessionBeginEnd::End { session_id }) => {
                let mut state = agent_state.lock().await;
                state
                    .session_connections
                    .entry(session_id.clone())
                    .and_modify(|c| *c = c.saturating_sub(1))
                    .or_insert(0);
                if let Some(c) = state.session_connections.get(&session_id)
                    && *c == 0
                {
                    info!("cleaning up session {session_id:?}");
                    state.mounted_shares.remove(&session_id);
                    state.session_connections.remove(&session_id);
                    if let Err(_e) =
                        state
                            .desired_state_channel
                            .send(StateUpdateToSession::MountUpdate {
                                session_id,
                                shares: None,
                            })
                    {
                        warn!("tried to send an update on current mounts but no one cared");
                    }
                }
            }
            None => {
                info!("session bookkeeper shutting down...");
                break;
            }
        }
    }
}

impl AgentState {
    pub async fn new(config: &AgentConfig) -> Arc<Mutex<Self>> {
        let config = config.clone();
        let (tx, _rx) = broadcast::channel(16);

        let (sess_tx, sess_rx) = mpsc::unbounded_channel::<SessionBeginEnd>();

        let state = Arc::new(Mutex::new(Self {
            connection: Default::default(),
            current_shares: Default::default(),
            remote_info: None,
            config,
            desired_state_channel: tx,
            mounted_shares: Default::default(),
            session_connections: Default::default(),
            session_channel_sender: sess_tx,
        }));

        // FIXME: error handling for spawning, baby sitting to keep it alive?
        let _join_handle = tokio::spawn(session_bookkeeper(Arc::clone(&state), sess_rx));

        state
    }
}
