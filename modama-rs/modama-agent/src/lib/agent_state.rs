use std::{collections::HashMap, sync::Arc};

use log::{info, warn};
use modama_common::{
    agent::AgentId,
    foreground::{
        OtherMount, ShareDestinationError, agent_mount_status, foreground_projection,
        other_mounted_shares,
    },
    internal_api_types::{ConnectedAgent, ConnectedAgents, SessionHelperState, Shares},
    local_api_messages::APICurrentForegroundShare,
    messages::{AgentMountStatus, AgentMountStatusEntry, Share},
};
use tokio::sync::{Mutex, Notify, broadcast, mpsc};

use crate::{
    caller_scope::HelperScope,
    config::{AgentConfig, MTLSIdentity},
    events_api::SessionBeginEnd,
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

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub connected_agents: Vec<AgentId>,

    /// Mount status of the connected agents that reported one, including ourselves
    pub agent_mount_status: Vec<AgentMountStatusEntry>,

    pub desired_shares: ShareState<Share>,
}

impl RemoteInfo {
    pub fn new() -> Self {
        Self {
            connected_agents: Default::default(),
            agent_mount_status: Default::default(),
            desired_shares: Default::default(),
        }
    }

    /// The desired shares, as sent to session helpers and GUIs
    pub fn shares(&self) -> Shares {
        Shares {
            foreground_share_id: self
                .desired_shares
                .foreground_share
                .as_ref()
                .map(|s| s.share_id.clone()),
            shares: self.desired_shares.shares.clone(),
        }
    }

    /// The last mount status `agent` reported to smbdrop; None if it didn't (yet)
    pub fn mount_status_of(&self, agent: &AgentId) -> Option<AgentMountStatus> {
        self.agent_mount_status
            .iter()
            .find(|entry| &entry.agent == agent)
            .map(|entry| entry.status)
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
        scope: HelperScope,
        /// In case the session helper disconnects, this is not an empty report
        /// but None! Meaning the GUI can distinguish these two cases if needed.
        session_helper: Option<SessionHelperState>,
    },
    /// The GUI of `scope` asked to force the release of mounts that are no longer desired; only
    /// that scope's helper acts on it
    ForceRelease { scope: HelperScope },
}

#[derive(Debug)]
pub struct AgentState {
    pub connection: ConnectionState,

    /// Per-scope last report of the session helper
    pub session_helpers: HashMap<HelperScope, SessionHelperState>,

    /// Counter for active session helper connections
    /// (to prevent cleanup while there's still an active connection)
    pub session_connections: HashMap<HelperScope, u64>,

    pub session_channel_sender: mpsc::UnboundedSender<SessionBeginEnd>,

    /// Info we received over the event channel; None if we haven't received any yet
    pub remote_info: Option<RemoteInfo>,

    /// The identity of the token of the current event channel connection: our own, or for an
    /// anonymous agent, that of the agent it borrowed the token from. None while not connected.
    pub token_identity: Option<AgentId>,

    /// Runtime configuration
    pub config: AgentConfig,

    /// Broadcast channel to update GUI and session helpers
    pub desired_state_channel: broadcast::Sender<StateUpdateToSession>,

    /// Notified when something `mount_status` depends on changed, apart from the desired state
    /// (which the event channel loop, the one waiter, updates itself): a session helper
    /// connected, reported, or went away
    pub mount_status_inputs_changed: Arc<Notify>,
}

/// Cleanup task: when connections for a session_id drop to zero, clear the session state
async fn session_bookkeeper(
    agent_state: Arc<Mutex<AgentState>>,
    mut channel: mpsc::UnboundedReceiver<SessionBeginEnd>,
) {
    loop {
        match channel.recv().await {
            Some(SessionBeginEnd::Begin { scope }) => {
                let mut state = agent_state.lock().await;
                state
                    .session_connections
                    .entry(scope)
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
                state.mount_status_inputs_changed.notify_one();
            }
            Some(SessionBeginEnd::End { scope }) => {
                let mut state = agent_state.lock().await;
                state
                    .session_connections
                    .entry(scope)
                    .and_modify(|c| *c = c.saturating_sub(1))
                    .or_insert(0);
                if let Some(c) = state.session_connections.get(&scope)
                    && *c == 0
                {
                    info!("cleaning up scope {scope:?}");
                    state.session_helpers.remove(&scope);
                    state.session_connections.remove(&scope);
                    if let Err(_e) =
                        state
                            .desired_state_channel
                            .send(StateUpdateToSession::MountUpdate {
                                scope,
                                session_helper: None,
                            })
                    {
                        warn!("tried to send an update on current mounts but no one cared");
                    }
                    state.mount_status_inputs_changed.notify_one();
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
    /// The foreground share as seen from `scope`: desired state combined with what that scope's
    /// session helper reported
    pub fn foreground_for(
        &self,
        scope: HelperScope,
    ) -> Result<APICurrentForegroundShare, ShareDestinationError> {
        let desired = self
            .remote_info
            .as_ref()
            .and_then(|remote| remote.desired_shares.foreground_share.as_ref());
        foreground_projection(desired, self.session_helpers.get(&scope))
    }

    /// Our mount status, over all connected session helpers; None as long as we haven't heard
    /// from smbdrop what should be mounted
    pub fn mount_status(&self) -> Option<AgentMountStatus> {
        let desired = self.remote_info.as_ref()?.shares();
        let helpers = self
            .session_connections
            .keys()
            .map(|scope| self.session_helpers.get(scope));
        Some(agent_mount_status(&desired, helpers))
    }

    /// The desired shares besides the foreground one that `scope`'s session helper has mounted
    pub fn other_mounts_for(&self, scope: HelperScope) -> Vec<OtherMount> {
        let (Some(remote), Some(helper)) =
            (self.remote_info.as_ref(), self.session_helpers.get(&scope))
        else {
            return Vec::new();
        };
        other_mounted_shares(&remote.shares(), helper)
    }

    /// The agents connected at the resource of our token; None until we are connected and
    /// smbdrop has told us
    pub fn connected_agents_here(&self) -> Option<ConnectedAgents> {
        connected_agents_at(
            &self.config.identity,
            self.token_identity.as_ref(),
            self.remote_info.as_ref(),
            self.mount_status(),
        )
    }

    pub async fn new(config: &AgentConfig) -> Arc<Mutex<Self>> {
        let config = config.clone();
        let (tx, _rx) = broadcast::channel(16);

        let (sess_tx, sess_rx) = mpsc::unbounded_channel::<SessionBeginEnd>();

        let state = Arc::new(Mutex::new(Self {
            connection: Default::default(),
            remote_info: None,
            token_identity: None,
            config,
            desired_state_channel: tx,
            session_helpers: Default::default(),
            session_connections: Default::default(),
            session_channel_sender: sess_tx,
            mount_status_inputs_changed: Default::default(),
        }));

        // FIXME: error handling for spawning, baby sitting to keep it alive?
        let _join_handle = tokio::spawn(session_bookkeeper(Arc::clone(&state), sess_rx));

        state
    }
}

/// The agents in `remote` at the resource of `token_identity`, with this agent (`identity`, with
/// `own_status`) on its own
fn connected_agents_at(
    identity: &MTLSIdentity,
    token_identity: Option<&AgentId>,
    remote: Option<&RemoteInfo>,
    own_status: Option<AgentMountStatus>,
) -> Option<ConnectedAgents> {
    let resource = &token_identity?.resource;
    let remote = remote?;
    let own = match identity {
        MTLSIdentity::AgentId { agent_name, .. } => ConnectedAgent {
            name: agent_name.clone(),
            verified: true,
            status: own_status,
        },
        MTLSIdentity::Anonymous { hostname } => ConnectedAgent {
            name: hostname.clone(),
            verified: false,
            status: own_status,
        },
    };
    let mut others: Vec<ConnectedAgent> = remote
        .connected_agents
        .iter()
        .filter(|agent| &agent.resource == resource)
        // a certified agent is in the list itself; an anonymous one isn't, it connects with the
        // identity it borrowed its token from
        .filter(|agent| !(own.verified && agent.agent == own.name))
        .map(|agent| ConnectedAgent {
            name: agent.agent.clone(),
            verified: true,
            status: remote.mount_status_of(agent),
        })
        .collect();
    others.sort_by(|a, b| a.name.cmp(&b.name));
    others.dedup();
    Some(ConnectedAgents {
        resource: resource.clone(),
        own,
        others,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(resource: &str, name: &str) -> AgentId {
        AgentId {
            resource: resource.to_owned(),
            agent: name.to_owned(),
            san_encoded: format!("x-modama-agent://{resource}#{name}"),
        }
    }

    fn verified(name: &str) -> ConnectedAgent {
        ConnectedAgent {
            name: name.to_owned(),
            verified: true,
            status: None,
        }
    }

    fn remote(connected: &[AgentId]) -> RemoteInfo {
        RemoteInfo {
            connected_agents: connected.to_vec(),
            ..RemoteInfo::default()
        }
    }

    fn certified() -> MTLSIdentity {
        MTLSIdentity::AgentId {
            agent_name: "k2".to_owned(),
            resource_name: "Holo".to_owned(),
        }
    }

    #[test]
    fn a_certified_agent_lists_the_others_at_its_resource() {
        let connected = [
            agent("Holo", "tem-camera"),
            agent("Other", "elsewhere"),
            agent("Holo", "k2"),
            agent("Holo", "detector"),
        ];
        let agents = connected_agents_at(
            &certified(),
            Some(&agent("Holo", "k2")),
            Some(&remote(&connected)),
            None,
        )
        .unwrap();
        assert_eq!(agents.resource, "Holo");
        assert_eq!(agents.own, verified("k2"));
        assert_eq!(
            agents.others,
            vec![verified("detector"), verified("tem-camera")]
        );
    }

    #[test]
    fn a_certified_agent_alone_has_no_others() {
        let connected = [agent("Holo", "k2")];
        let agents = connected_agents_at(
            &certified(),
            Some(&agent("Holo", "k2")),
            Some(&remote(&connected)),
            None,
        )
        .unwrap();
        assert!(agents.others.is_empty());
    }

    #[test]
    fn an_anonymous_agent_inherits_the_resource_of_its_token() {
        let anonymous = MTLSIdentity::Anonymous {
            hostname: "workstation".to_owned(),
        };
        let connected = [agent("Holo", "k2"), agent("Other", "elsewhere")];
        let agents = connected_agents_at(
            &anonymous,
            Some(&agent("Holo", "k2")),
            Some(&remote(&connected)),
            None,
        )
        .unwrap();
        assert_eq!(agents.resource, "Holo");
        assert_eq!(
            agents.own,
            ConnectedAgent {
                name: "workstation".to_owned(),
                verified: false,
                status: None,
            }
        );
        // the agent it borrowed the token from is one of the others
        assert_eq!(agents.others, vec![verified("k2")]);
    }

    #[test]
    fn unknown_until_connected_and_told() {
        let connected = [agent("Holo", "k2")];
        assert!(connected_agents_at(&certified(), None, Some(&remote(&connected)), None).is_none());
        assert!(
            connected_agents_at(&certified(), Some(&agent("Holo", "k2")), None, None).is_none()
        );
    }
}
