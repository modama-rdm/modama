use log::warn;
use modama_common::{agent::AgentId, messages::Share};

use crate::{config::AgentConfig, share::MountedShare};

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
        if let Some(share) = &self.foreground_share {
            if !self.shares.contains(share) {
                warn!("correcting inconsistent state: foreground_share {:?} was not part of the share list", share);
                self.shares.push(share.clone());
            }
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

#[derive(Debug)]
pub struct AgentState {
    pub connection: ConnectionState,

    /// The currently mounted shares, according to the last update
    pub current_shares: ShareState<MountedShare>,

    /// Info we received over the event channel; None if we haven't received any yet
    pub remote_info: Option<RemoteInfo>,

    /// Runtime configuration
    pub config: AgentConfig,
}

impl AgentState {
    pub fn new(config: &AgentConfig) -> Self {
        let config = config.clone();
        Self {
            connection: Default::default(),
            current_shares: Default::default(),
            remote_info: None,
            config,
        }
    }
}
