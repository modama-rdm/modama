use modama_common::{agent::AgentId, messages::Share};

use crate::{config::AgentConfig, share::MountedShare};

#[derive(Debug, Default)]
pub enum ConnectionState {
    #[default]
    Connecting,
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
    T: PartialEq + Clone,
{
    pub fn set_foreground_share(&mut self, share: Option<T>) {
        self.foreground_share = share;
        self.ensure_consistent();
    }

    pub fn ensure_consistent(&mut self) {
        // ensure the foreground share is part of the list of all shares:
        if let Some(share) = &self.foreground_share {
            if !self.shares.contains(share) {
                self.shares.push(share.clone());
            }
        }
    }

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




#[derive(Debug)]
pub struct AgentState {
    pub connection: ConnectionState,

    pub current_shares: ShareState<MountedShare>,
    pub desired_shares: ShareState<Share>,

    pub connected_agents: Vec<AgentId>,

    /// Runtime configuration
    pub config: AgentConfig,
}

impl AgentState {
    pub fn new(config: &AgentConfig) -> Self {
        let config = config.clone();
        Self {
            connection: Default::default(),
            current_shares: Default::default(),
            desired_shares: Default::default(),
            connected_agents: Default::default(),
            config,
        }
    }
}

