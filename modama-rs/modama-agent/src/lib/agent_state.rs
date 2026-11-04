use std::sync::Arc;

use log::warn;
use tokio::sync::Mutex;

use crate::{config::AgentConfig, local_fan_out::LocalFanOut};

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

#[derive(Debug)]
pub struct AgentState {
    pub connection: ConnectionState,

    /// What is fanned out to the session helpers and GUIs on this computer
    pub fan_out: Arc<LocalFanOut>,

    /// Runtime configuration
    pub config: AgentConfig,
}

impl AgentState {
    pub fn new(config: &AgentConfig) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            connection: Default::default(),
            fan_out: Arc::new(LocalFanOut::new(config.identity.clone())),
            config: config.clone(),
        }))
    }
}
