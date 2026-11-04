//! What this agent fans out to the session helpers and GUIs on this computer: the desired
//! shares from smbdrop, the agents connected at our resource, and per scope the presence and
//! last report of the session helper. Everything lives in one value behind one lock (the
//! `watch` channel's); every change notifies the subscribers, which derive what concerns them
//! from the current value. A subscriber that falls behind skips intermediate values, but never
//! the latest one. A change and its notification are one step, so callers can't get their
//! order wrong.

use std::{collections::HashMap, sync::Arc};

use log::info;
use modama_common::{
    agent::AgentId,
    foreground::{OtherMount, ScopeView, ShareDestinationError, agent_mount_status},
    internal_api_types::{
        ConnectedAgent, ConnectedAgents, ConnectionDetails, SessionHelperState, Shares,
        ToSessionGui,
    },
    local_api_messages::APICurrentForegroundShare,
    messages::{AgentMountStatus, ConnectionId, Participant, ParticipantIdentity, Share},
};
use tokio::sync::watch;

use crate::{agent_state::ShareState, caller_scope::HelperScope, config::MTLSIdentity};

/// The fan-out to local session helpers and GUIs; see the module documentation
#[derive(Debug)]
pub struct LocalFanOut {
    state: watch::Sender<FanOutState>,
}

/// What smbdrop told us last
#[derive(Debug, Clone)]
struct RemoteInfo {
    /// The connections to smbdrop at our resource, ours included
    participants: Vec<Participant>,

    desired_shares: ShareState<Share>,
}

impl RemoteInfo {
    /// The desired shares, as sent to session helpers and GUIs
    fn shares(&self) -> Shares {
        Shares {
            foreground_share_id: self
                .desired_shares
                .foreground_share
                .as_ref()
                .map(|s| s.share_id.clone()),
            shares: self.desired_shares.shares.clone(),
        }
    }

    /// The participant on `connection`; None if smbdrop doesn't list it
    fn participant(&self, connection: ConnectionId) -> Option<&Participant> {
        self.participants
            .iter()
            .find(|participant| participant.connection == connection)
    }
}

/// The session helper connections of one scope
#[derive(Debug, Default)]
struct ScopeHelpers {
    /// Open connections; a scope is only listed while it has at least one
    connections: u64,

    /// The last report from any of them; None until one reported
    report: Option<SessionHelperState>,
}

#[derive(Debug)]
struct FanOutState {
    /// This agent's own identity
    identity: MTLSIdentity,

    /// The identity of the token of the current event channel connection: our own, or for an
    /// anonymous agent, that of the agent it borrowed the token from. None while not connected.
    token_identity: Option<AgentId>,

    /// The ID smbdrop gave our current event channel connection in its `Welcome`; None while
    /// not connected
    own_connection: Option<ConnectionId>,

    /// What smbdrop told us; None if we haven't heard from it yet
    remote: Option<RemoteInfo>,

    /// Counts the changes of the desired shares, so session helpers learn only of those, not of
    /// smbdrop resending them with other participants
    desired_version: u64,

    /// The scopes with connected session helpers
    helpers: HashMap<HelperScope, ScopeHelpers>,

    /// Per scope, how often its GUI asked to force the release of obsolete mounts. Only grows: a
    /// helper compares it with the count when it connected, so it acts on every request made
    /// while it is connected and on none made before.
    force_release_requests: HashMap<HelperScope, u64>,

    /// Per scope, how often its last session helper went away. A GUI launches a session helper
    /// when told its scope has none, so it hears of a departure even when what it shows stays
    /// the same, for example when the helper never reported.
    helper_departures: HashMap<HelperScope, u64>,
}

/// Whether two desired states are the same; the order of the shares doesn't matter, as
/// smbdrop doesn't keep a stable order for shares created at the same time
fn same_desired(a: &ShareState<Share>, b: &ShareState<Share>) -> bool {
    let sorted = |state: &ShareState<Share>| {
        let mut shares = state.shares.clone();
        shares.sort_by(|s1, s2| s1.share_id.cmp(&s2.share_id));
        shares
    };
    a.foreground_share == b.foreground_share && sorted(a) == sorted(b)
}

impl FanOutState {
    fn desired_shares(&self) -> Option<Shares> {
        self.remote.as_ref().map(RemoteInfo::shares)
    }

    fn report(&self, scope: HelperScope) -> Option<&SessionHelperState> {
        self.helpers.get(&scope)?.report.as_ref()
    }

    fn force_release_requests(&self, scope: HelperScope) -> u64 {
        self.force_release_requests
            .get(&scope)
            .copied()
            .unwrap_or(0)
    }

    fn mount_status(&self) -> Option<AgentMountStatus> {
        let desired = self.remote.as_ref()?.shares();
        let scopes = self
            .helpers
            .values()
            .map(|helpers| ScopeView::new(Some(&desired), helpers.report.as_ref()));
        Some(agent_mount_status(scopes))
    }

    fn connected_agents(&self) -> Option<ConnectedAgents> {
        connected_agents_at(
            &self.identity,
            self.token_identity.as_ref(),
            self.own_connection,
            self.remote.as_ref(),
            self.mount_status(),
        )
    }

    fn gui_view(&self, scope: HelperScope) -> GuiView {
        GuiView {
            session_helper: self.report(scope).cloned(),
            desired_shares: self.desired_shares(),
            connected_agents: self.connected_agents(),
            helper_departures: self.helper_departures.get(&scope).copied().unwrap_or(0),
        }
    }
}

impl LocalFanOut {
    /// Nothing heard from smbdrop yet, no session helpers
    pub fn new(identity: MTLSIdentity) -> Self {
        let (state, _) = watch::channel(FanOutState {
            identity,
            token_identity: None,
            own_connection: None,
            remote: None,
            desired_version: 0,
            helpers: HashMap::new(),
            force_release_requests: HashMap::new(),
            helper_departures: HashMap::new(),
        });
        Self { state }
    }

    // smbdrop's side, from the event channel

    /// We are not connected to smbdrop (anymore); what it last told us stays desired
    pub fn smbdrop_disconnected(&self) {
        self.state.send_if_modified(|state| {
            let was_connected = state.token_identity.is_some() || state.own_connection.is_some();
            state.token_identity = None;
            state.own_connection = None;
            was_connected
        });
    }

    /// We logged in to smbdrop with a token of `token_identity`
    pub fn logged_in(&self, token_identity: AgentId) {
        self.state
            .send_modify(|state| state.token_identity = Some(token_identity));
    }

    /// smbdrop welcomed us on `connection`
    pub fn welcomed(&self, connection: ConnectionId) {
        self.state
            .send_modify(|state| state.own_connection = Some(connection));
    }

    /// smbdrop sent its share list. It also does so when only the participants or their status
    /// changed; session helpers would run a reconciliation pass for nothing, so they only hear of
    /// it when the desired shares changed. GUIs show the participants as well.
    pub fn apply_share_list(
        &self,
        shares: Vec<Share>,
        foreground_share: Option<Share>,
        participants: Vec<Participant>,
    ) {
        let desired_shares = ShareState {
            foreground_share,
            shares,
        };
        self.state
            .send_if_modified(|state| match &mut state.remote {
                Some(remote) => {
                    // the same shares in another order stay in the order we already have
                    let desired_changed = !same_desired(&remote.desired_shares, &desired_shares);
                    let participants_changed = remote.participants != participants;
                    if desired_changed {
                        remote.desired_shares = desired_shares;
                        state.desired_version += 1;
                    }
                    if participants_changed {
                        remote.participants = participants;
                    }
                    desired_changed || participants_changed
                }
                None => {
                    state.remote = Some(RemoteInfo {
                        participants,
                        desired_shares,
                    });
                    state.desired_version += 1;
                    true
                }
            });
    }

    // the local side: session helpers and GUIs

    /// A session helper of `scope` connected: it counts as present until the returned
    /// `HelperPresence` is dropped, and the `HelperFeed` tells it what to do, starting with the
    /// current desired state
    pub fn connect_helper(self: &Arc<Self>, scope: HelperScope) -> (HelperPresence, HelperFeed) {
        let mut force_releases_seen = 0;
        self.state.send_if_modified(|state| {
            force_releases_seen = state.force_release_requests(scope);
            let helpers = state.helpers.entry(scope).or_default();
            helpers.connections += 1;
            // another connection of a present scope changes nobody's view
            helpers.connections == 1
        });
        let presence = HelperPresence {
            fan_out: Arc::clone(self),
            scope,
        };
        let feed = HelperFeed {
            changes: self.state.subscribe(),
            scope,
            sent_desired_version: None,
            force_releases_seen,
        };
        (presence, feed)
    }

    /// A GUI of `scope` connected; the feed starts with what it shows now
    pub fn subscribe_gui(&self, scope: HelperScope) -> GuiFeed {
        GuiFeed {
            changes: self.state.subscribe(),
            scope,
            sent: None,
        }
    }

    /// The GUI of `scope` asked to force the release of mounts that are no longer desired; the
    /// session helpers of `scope` connected now act on it, those connecting later don't
    pub fn request_force_release(&self, scope: HelperScope) {
        self.state.send_modify(|state| {
            *state.force_release_requests.entry(scope).or_insert(0) += 1;
        });
    }

    /// Our mount status as it changes, for reporting it to smbdrop
    pub fn mount_status_feed(&self) -> MountStatusFeed {
        MountStatusFeed {
            changes: self.state.subscribe(),
            seen: None,
        }
    }

    // queries for the local HTTP API

    /// Our mount status, over all connected session helpers; None as long as we haven't heard
    /// from smbdrop what should be mounted
    pub fn mount_status(&self) -> Option<AgentMountStatus> {
        self.state.borrow().mount_status()
    }

    /// The desired foreground share, mounted or not
    pub fn desired_foreground(&self) -> Option<Share> {
        let desired = self.state.borrow().desired_shares()?;
        // the share `ScopeView::foreground` reports, so the two can't disagree
        ScopeView::new(Some(&desired), None)
            .foreground_share()
            .cloned()
    }

    /// The foreground share as seen from `scope`: desired state combined with what that scope's
    /// session helper reported
    pub fn foreground_for(
        &self,
        scope: HelperScope,
    ) -> Result<APICurrentForegroundShare, ShareDestinationError> {
        let state = self.state.borrow();
        let desired = state.desired_shares();
        ScopeView::new(desired.as_ref(), state.report(scope)).foreground()
    }

    /// The desired shares besides the foreground one that `scope`'s session helper has mounted
    pub fn other_mounts_for(&self, scope: HelperScope) -> Vec<OtherMount> {
        let state = self.state.borrow();
        let desired = state.desired_shares();
        ScopeView::new(desired.as_ref(), state.report(scope)).other_mounts()
    }

    /// The agents connected at the resource of our token; None until we are connected and
    /// smbdrop has told us
    pub fn connected_agents(&self) -> Option<ConnectedAgents> {
        self.state.borrow().connected_agents()
    }

    /// Our mount status as smbdrop lists it back to us
    #[cfg(test)]
    pub(crate) fn own_status_at_smbdrop(&self) -> Option<AgentMountStatus> {
        let state = self.state.borrow();
        let own = state.own_connection?;
        state.remote.as_ref()?.participant(own)?.status
    }
}

/// A connected session helper; while it lives, its scope counts as having one
#[derive(Debug)]
pub struct HelperPresence {
    fan_out: Arc<LocalFanOut>,
    scope: HelperScope,
}

impl HelperPresence {
    /// The helper reported what it has mounted
    pub fn report(&self, report: SessionHelperState) {
        self.fan_out.state.send_if_modified(|state| {
            match state.helpers.get_mut(&self.scope) {
                Some(helpers) if helpers.report.as_ref() != Some(&report) => {
                    helpers.report = Some(report);
                    true
                }
                // unchanged; or the scope has no entry, which can't happen while this presence
                // exists
                Some(_) | None => false,
            }
        });
    }
}

impl Drop for HelperPresence {
    fn drop(&mut self) {
        let scope = self.scope;
        self.fan_out.state.send_if_modified(|state| {
            let Some(helpers) = state.helpers.get_mut(&scope) else {
                return false;
            };
            helpers.connections = helpers.connections.saturating_sub(1);
            if helpers.connections == 0 {
                info!("the last session helper of {scope:?} went away");
                state.helpers.remove(&scope);
                *state.helper_departures.entry(scope).or_insert(0) += 1;
            }
            // a remaining connection changes nobody's view; the last one leaving does
            !state.helpers.contains_key(&scope)
        });
    }
}

/// What a session helper is told: the desired shares, and whether to force the release of
/// obsolete mounts
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperUpdate {
    /// None until smbdrop told us
    pub share_state: Option<Shares>,
    pub force_release_obsolete: bool,
}

/// What one session helper connection is to do: the desired state when it changed, and the
/// force-release requests of its scope's GUI
#[derive(Debug)]
pub struct HelperFeed {
    changes: watch::Receiver<FanOutState>,
    scope: HelperScope,
    /// None until we sent the first update
    sent_desired_version: Option<u64>,
    force_releases_seen: u64,
}

impl HelperFeed {
    /// The next update, if there is one now; the first is the current desired state
    pub fn try_next(&mut self) -> Option<HelperUpdate> {
        let state = self.changes.borrow_and_update();
        let requests = state.force_release_requests(self.scope);
        let force = requests > self.force_releases_seen;
        if !force && self.sent_desired_version == Some(state.desired_version) {
            return None;
        }
        self.sent_desired_version = Some(state.desired_version);
        self.force_releases_seen = requests;
        Some(HelperUpdate {
            share_state: state.desired_shares(),
            force_release_obsolete: force,
        })
    }

    /// Wait for the next update; cancel-safe
    pub async fn next(&mut self) -> HelperUpdate {
        loop {
            if let Some(update) = self.try_next() {
                return update;
            }
            wait_for_change(&mut self.changes).await;
        }
    }
}

/// What a GUI shows, to send only when it changed
#[derive(Debug, Clone, PartialEq, Eq)]
struct GuiView {
    session_helper: Option<SessionHelperState>,
    desired_shares: Option<Shares>,
    connected_agents: Option<ConnectedAgents>,
    /// Not shown, but a change is worth a message; see `FanOutState::helper_departures`
    helper_departures: u64,
}

impl From<GuiView> for ToSessionGui {
    fn from(view: GuiView) -> Self {
        Self {
            session_helper: view.session_helper,
            desired_shares: view.desired_shares,
            connected_agents: view.connected_agents,
        }
    }
}

/// What the GUIs of one scope show: the desired shares, the report of the scope's session
/// helper (None while it has none), and the connected agents
#[derive(Debug)]
pub struct GuiFeed {
    changes: watch::Receiver<FanOutState>,
    scope: HelperScope,
    /// None until we sent the first one
    sent: Option<GuiView>,
}

impl GuiFeed {
    /// What the GUI shows, if it changed since the last call; the first is what it shows now
    pub fn try_next(&mut self) -> Option<ToSessionGui> {
        let view = self.changes.borrow_and_update().gui_view(self.scope);
        if self.sent.as_ref() == Some(&view) {
            return None;
        }
        self.sent = Some(view.clone());
        Some(view.into())
    }

    /// Wait for the GUI's next view; cancel-safe
    pub async fn next(&mut self) -> ToSessionGui {
        loop {
            if let Some(view) = self.try_next() {
                return view;
            }
            wait_for_change(&mut self.changes).await;
        }
    }
}

/// Our mount status as it changes
#[derive(Debug)]
pub struct MountStatusFeed {
    changes: watch::Receiver<FanOutState>,
    /// None until the first call
    seen: Option<Option<AgentMountStatus>>,
}

impl MountStatusFeed {
    /// The mount status, if it changed since the last call; the first is the current one
    pub fn try_next(&mut self) -> Option<Option<AgentMountStatus>> {
        let status = self.changes.borrow_and_update().mount_status();
        if self.seen == Some(status) {
            return None;
        }
        self.seen = Some(status);
        Some(status)
    }

    /// Wait until the mount status changed; cancel-safe
    pub async fn next(&mut self) -> Option<AgentMountStatus> {
        loop {
            if let Some(status) = self.try_next() {
                return status;
            }
            wait_for_change(&mut self.changes).await;
        }
    }
}

/// Wait until the fan-out state changed. Every feed's receiver comes from a `LocalFanOut`,
/// whose sender lives as long as the state does; were it gone, there would be no change to
/// wait for.
async fn wait_for_change(changes: &mut watch::Receiver<FanOutState>) {
    if changes.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// How `participant` is connected, for the GUI
fn connection_details(participant: &Participant) -> ConnectionDetails {
    ConnectionDetails {
        id: participant.connection,
        peer: participant.peer,
        lender: match &participant.identity {
            ParticipantIdentity::Certified { .. } => None,
            ParticipantIdentity::Anonymous { lender, .. } => Some(lender.agent.clone()),
        },
    }
}

/// The participants in `remote`, which smbdrop limits to the resource of `token_identity`, with
/// this agent (`identity`, on `own_connection`, with `own_status`) on its own; None until smbdrop
/// welcomed us and listed the participants, this agent's connection among them
fn connected_agents_at(
    identity: &MTLSIdentity,
    token_identity: Option<&AgentId>,
    own_connection: Option<ConnectionId>,
    remote: Option<&RemoteInfo>,
    own_status: Option<AgentMountStatus>,
) -> Option<ConnectedAgents> {
    let resource = &token_identity?.resource;
    let own_connection = own_connection?;
    let remote = remote?;
    let (name, verified) = match identity {
        MTLSIdentity::AgentId { agent_name, .. } => (agent_name.clone(), true),
        MTLSIdentity::Anonymous { hostname } => (hostname.clone(), false),
    };
    let own = ConnectedAgent {
        name,
        verified,
        status: own_status,
        connection: connection_details(remote.participant(own_connection)?),
    };
    let mut others: Vec<ConnectedAgent> = remote
        .participants
        .iter()
        .filter(|participant| participant.connection != own_connection)
        .map(|participant| {
            let (name, verified) = match &participant.identity {
                ParticipantIdentity::Certified { agent } => (agent.agent.clone(), true),
                ParticipantIdentity::Anonymous { name, .. } => (name.clone(), false),
            };
            ConnectedAgent {
                name,
                verified,
                status: participant.status,
                connection: connection_details(participant),
            }
        })
        .collect();
    others.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.connection.id.cmp(&b.connection.id))
    });
    Some(ConnectedAgents {
        resource: resource.clone(),
        own,
        others,
    })
}

#[cfg(test)]
mod tests {
    use modama_common::internal_api_types::MountedShare;

    use super::*;

    fn agent(resource: &str, name: &str) -> AgentId {
        AgentId {
            resource: resource.to_owned(),
            agent: name.to_owned(),
            san_encoded: format!("x-modama-agent://{resource}#{name}"),
        }
    }

    fn certified_at(id: u64, resource: &str, name: &str) -> Participant {
        Participant {
            connection: ConnectionId(id),
            identity: ParticipantIdentity::Certified {
                agent: agent(resource, name),
            },
            peer: format!("192.0.2.{id}:50000").parse().unwrap(),
            status: None,
        }
    }

    fn anonymous_at(id: u64, name: &str, lender: &AgentId) -> Participant {
        Participant {
            connection: ConnectionId(id),
            identity: ParticipantIdentity::Anonymous {
                name: name.to_owned(),
                lender: lender.clone(),
            },
            peer: format!("192.0.2.{id}:50000").parse().unwrap(),
            status: None,
        }
    }

    fn remote(participants: &[Participant]) -> RemoteInfo {
        RemoteInfo {
            participants: participants.to_vec(),
            desired_shares: ShareState::default(),
        }
    }

    fn certified() -> MTLSIdentity {
        MTLSIdentity::AgentId {
            agent_name: "k2".to_owned(),
            resource_name: "Holo".to_owned(),
        }
    }

    fn anonymous() -> MTLSIdentity {
        MTLSIdentity::Anonymous {
            hostname: "workstation".to_owned(),
        }
    }

    /// (name, verified, connection ID, lender) of a listed agent
    fn summary(agent: &ConnectedAgent) -> (&str, bool, u64, Option<&str>) {
        (
            &agent.name,
            agent.verified,
            agent.connection.id.0,
            agent.connection.lender.as_deref(),
        )
    }

    fn others(agents: &ConnectedAgents) -> Vec<(&str, bool, u64, Option<&str>)> {
        agents.others.iter().map(summary).collect()
    }

    #[test]
    fn a_certified_agent_lists_the_others() {
        let participants = [
            certified_at(1, "Holo", "tem-camera"),
            certified_at(3, "Holo", "k2"),
            certified_at(4, "Holo", "detector"),
        ];
        let agents = connected_agents_at(
            &certified(),
            Some(&agent("Holo", "k2")),
            Some(ConnectionId(3)),
            Some(&remote(&participants)),
            None,
        )
        .unwrap();
        assert_eq!(agents.resource, "Holo");
        assert_eq!(summary(&agents.own), ("k2", true, 3, None));
        assert_eq!(agents.own.connection.peer.to_string(), "192.0.2.3:50000");
        assert_eq!(
            others(&agents),
            vec![("detector", true, 4, None), ("tem-camera", true, 1, None)]
        );
    }

    #[test]
    fn a_certified_agent_alone_has_no_others() {
        let agents = connected_agents_at(
            &certified(),
            Some(&agent("Holo", "k2")),
            Some(ConnectionId(3)),
            Some(&remote(&[certified_at(3, "Holo", "k2")])),
            None,
        )
        .unwrap();
        assert!(agents.others.is_empty());
    }

    /// An anonymous agent takes part at its lender's resource; the lender is listed as itself,
    /// and the anonymous agent's own connection only as its own row
    #[test]
    fn an_anonymous_agent_is_listed_apart_from_its_lender() {
        let k2 = agent("Holo", "k2");
        let participants = [
            certified_at(1, "Holo", "k2"),
            anonymous_at(2, "workstation", &k2),
        ];
        let agents = connected_agents_at(
            &anonymous(),
            Some(&k2),
            Some(ConnectionId(2)),
            Some(&remote(&participants)),
            None,
        )
        .unwrap();
        assert_eq!(agents.resource, "Holo");
        assert_eq!(summary(&agents.own), ("workstation", false, 2, Some("k2")));
        assert_eq!(others(&agents), vec![("k2", true, 1, None)]);

        // and the lender sees the anonymous agent
        let agents = connected_agents_at(
            &certified(),
            Some(&k2),
            Some(ConnectionId(1)),
            Some(&remote(&participants)),
            None,
        )
        .unwrap();
        assert_eq!(others(&agents), vec![("workstation", false, 2, Some("k2"))]);
    }

    /// Two connections under one name, for example our own old connection that smbdrop still
    /// holds after a reconnect: only our current one is our own row
    #[test]
    fn other_connections_with_our_identity_are_others() {
        let participants = [certified_at(2, "Holo", "k2"), certified_at(5, "Holo", "k2")];
        let agents = connected_agents_at(
            &certified(),
            Some(&agent("Holo", "k2")),
            Some(ConnectionId(5)),
            Some(&remote(&participants)),
            None,
        )
        .unwrap();
        assert_eq!(summary(&agents.own), ("k2", true, 5, None));
        assert_eq!(others(&agents), vec![("k2", true, 2, None)]);
    }

    #[test]
    fn unknown_until_welcomed_and_told() {
        let participants = [certified_at(3, "Holo", "k2")];
        let k2 = agent("Holo", "k2");
        let remote = remote(&participants);
        let at = |token, own, remote| connected_agents_at(&certified(), token, own, remote, None);
        assert!(at(None, Some(ConnectionId(3)), Some(&remote)).is_none());
        assert!(at(Some(&k2), None, Some(&remote)).is_none());
        assert!(at(Some(&k2), Some(ConnectionId(3)), None).is_none());
        // listed, but not yet with our own connection
        assert!(at(Some(&k2), Some(ConnectionId(4)), Some(&remote)).is_none());
    }

    // the fan-out, as subscribers see it

    const DESKTOP_1: HelperScope = HelperScope::Uid(1001);
    const DESKTOP_2: HelperScope = HelperScope::Uid(1002);

    fn share(id: &str) -> Share {
        Share {
            resource: Some("Holo".to_owned()),
            share_id: id.to_owned(),
            share_name: format!("share-{id}"),
            dest_path: format!("/project/{id}"),
            created_timestamp: 0,
            username: "share-user".to_owned(),
            password: "share-password".to_owned(),
            uid: 1000,
            gid: 1000,
            created_by: None,
        }
    }

    fn fan_out() -> Arc<LocalFanOut> {
        Arc::new(LocalFanOut::new(certified()))
    }

    /// smbdrop desires `ids`, the last one in the foreground, with `participants`
    fn smbdrop_sends(fan_out: &LocalFanOut, ids: &[&str], participants: &[Participant]) {
        let shares: Vec<Share> = ids.iter().map(|id| share(id)).collect();
        fan_out.apply_share_list(
            shares.clone(),
            shares.last().cloned(),
            participants.to_vec(),
        );
    }

    /// The share IDs of a desired state, None if there is none
    fn ids(shares: Option<&Shares>) -> Option<Vec<&str>> {
        shares.map(|shares| shares.shares.iter().map(|s| s.share_id.as_str()).collect())
    }

    /// The share IDs a helper update tells it to mount
    fn helper_ids(update: &HelperUpdate) -> Option<Vec<&str>> {
        ids(update.share_state.as_ref())
    }

    fn report(mounted: &[&str], reconciled: &[&str]) -> SessionHelperState {
        SessionHelperState {
            mounted_shares: mounted
                .iter()
                .map(|id| MountedShare {
                    share_name: format!("share-{id}"),
                    mount_path: Some(format!("/mnt/{id}").into()),
                })
                .collect(),
            errors: vec![],
            reconciled_share_ids: Some(reconciled.iter().map(|id| (*id).to_owned()).collect()),
        }
    }

    #[test]
    fn a_helper_first_gets_the_current_desired_state() {
        let fan_out = fan_out();
        let (_presence, mut early) = fan_out.connect_helper(DESKTOP_1);
        let update = early.try_next().unwrap();
        assert_eq!(update.share_state, None, "nothing heard from smbdrop yet");
        assert!(!update.force_release_obsolete);
        assert_eq!(early.try_next(), None);

        smbdrop_sends(&fan_out, &["a", "b"], &[]);
        let (_presence, mut late) = fan_out.connect_helper(DESKTOP_1);
        let update = late.try_next().unwrap();
        assert_eq!(helper_ids(&update), Some(vec!["a", "b"]));
        assert_eq!(
            update.share_state.unwrap().foreground_share_id.as_deref(),
            Some("b")
        );
        assert_eq!(late.try_next(), None);
    }

    /// A subscriber that doesn't keep up skips to the latest state; it can't miss it
    #[test]
    fn a_lagging_helper_gets_the_latest_desired_state_once() {
        let fan_out = fan_out();
        let (_presence, mut helper) = fan_out.connect_helper(DESKTOP_1);
        helper.try_next();

        for ids in [&["a"][..], &["a", "b"], &["b"], &["b", "c"]] {
            smbdrop_sends(&fan_out, ids, &[]);
        }
        assert_eq!(
            helper.try_next().as_ref().and_then(helper_ids),
            Some(vec!["b", "c"])
        );
        assert_eq!(helper.try_next(), None);
    }

    #[test]
    fn helpers_only_hear_of_changed_shares_guis_also_of_participants() {
        let fan_out = fan_out();
        fan_out.logged_in(agent("Holo", "k2"));
        fan_out.welcomed(ConnectionId(1));
        let own = certified_at(1, "Holo", "k2");
        smbdrop_sends(&fan_out, &["a", "b"], std::slice::from_ref(&own));
        let (_presence, mut helper) = fan_out.connect_helper(DESKTOP_1);
        let mut gui = fan_out.subscribe_gui(DESKTOP_1);
        helper.try_next();
        let first = gui.try_next().unwrap();
        assert_eq!(first.connected_agents.unwrap().others, vec![]);

        // the same shares, in another order: nothing new for anybody
        let reordered = vec![share("b"), share("a")];
        fan_out.apply_share_list(reordered, Some(share("b")), vec![own.clone()]);
        assert_eq!(helper.try_next(), None);
        assert!(gui.try_next().is_none());

        // another agent connects
        let other = certified_at(2, "Holo", "detector");
        smbdrop_sends(&fan_out, &["a", "b"], &[own.clone(), other.clone()]);
        assert_eq!(helper.try_next(), None);
        let message = gui.try_next().unwrap();
        assert_eq!(message.connected_agents.unwrap().others.len(), 1);
        assert_eq!(ids(message.desired_shares.as_ref()), Some(vec!["a", "b"]));

        // the shares change
        smbdrop_sends(&fan_out, &["a"], &[own, other]);
        assert_eq!(
            helper.try_next().as_ref().and_then(helper_ids),
            Some(vec!["a"])
        );
        let message = gui.try_next().unwrap();
        assert_eq!(ids(message.desired_shares.as_ref()), Some(vec!["a"]));
    }

    #[test]
    fn a_guis_scope_sees_its_helpers_report_and_departure() {
        let fan_out = fan_out();
        smbdrop_sends(&fan_out, &["a"], &[]);
        let mut gui = fan_out.subscribe_gui(DESKTOP_1);
        let mut other_gui = fan_out.subscribe_gui(DESKTOP_2);
        assert_eq!(gui.try_next().unwrap().session_helper, None);
        other_gui.try_next();

        let (presence, _feed) = fan_out.connect_helper(DESKTOP_1);
        // connected, but without a report there is nothing to show
        assert!(gui.try_next().is_none());

        presence.report(report(&["a"], &["a"]));
        let message = gui.try_next().unwrap();
        assert_eq!(message.session_helper, Some(report(&["a"], &["a"])));
        assert_eq!(ids(message.desired_shares.as_ref()), Some(vec!["a"]));
        assert!(other_gui.try_next().is_none(), "another scope's report");

        // the same report again
        presence.report(report(&["a"], &["a"]));
        assert!(gui.try_next().is_none());

        // a second connection of the scope comes and goes; the report stays
        let second = fan_out.connect_helper(DESKTOP_1);
        drop(second);
        assert!(gui.try_next().is_none());

        // the last one goes: no report, rather than an empty one
        drop(presence);
        assert_eq!(gui.try_next().unwrap().session_helper, None);
    }

    /// A GUI relaunches a missing helper when told there is no report; it is told also when a
    /// helper that never reported goes away, which changes nothing it shows
    #[test]
    fn a_guis_scope_hears_when_its_helper_goes_away_without_reporting() {
        let fan_out = fan_out();
        let mut gui = fan_out.subscribe_gui(DESKTOP_1);
        let mut other_gui = fan_out.subscribe_gui(DESKTOP_2);
        gui.try_next();
        other_gui.try_next();

        let (presence, _feed) = fan_out.connect_helper(DESKTOP_1);
        let (second, _second_feed) = fan_out.connect_helper(DESKTOP_1);
        assert!(gui.try_next().is_none());
        drop(presence);
        assert!(
            gui.try_next().is_none(),
            "another helper of the scope is left"
        );

        drop(second);
        let message = gui.try_next().unwrap();
        assert_eq!(message.session_helper, None);
        assert_eq!(message.desired_shares, None);
        assert!(gui.try_next().is_none());
        assert!(other_gui.try_next().is_none(), "another scope's helper");
    }

    #[test]
    fn a_force_release_reaches_the_helpers_of_its_scope_connected_now() {
        let fan_out = fan_out();
        smbdrop_sends(&fan_out, &["a"], &[]);
        let (_p1, mut own) = fan_out.connect_helper(DESKTOP_1);
        let (_p2, mut own_second) = fan_out.connect_helper(DESKTOP_1);
        let (_p3, mut other) = fan_out.connect_helper(DESKTOP_2);
        for feed in [&mut own, &mut own_second, &mut other] {
            feed.try_next();
        }

        fan_out.request_force_release(DESKTOP_1);
        for feed in [&mut own, &mut own_second] {
            let update = feed.try_next().unwrap();
            assert!(update.force_release_obsolete);
            assert_eq!(helper_ids(&update), Some(vec!["a"]));
            assert_eq!(feed.try_next(), None, "once");
        }
        assert_eq!(other.try_next(), None);

        // a helper connecting later doesn't act on it
        let (_p4, mut later) = fan_out.connect_helper(DESKTOP_1);
        assert!(!later.try_next().unwrap().force_release_obsolete);

        // nor does the next desired state carry it
        smbdrop_sends(&fan_out, &["b"], &[]);
        let update = own.try_next().unwrap();
        assert!(!update.force_release_obsolete);
        assert_eq!(helper_ids(&update), Some(vec!["b"]));
    }

    /// A request is not lost to a helper that is behind; it comes with the latest state
    #[test]
    fn a_force_release_coming_with_a_new_desired_state_is_one_update() {
        let fan_out = fan_out();
        smbdrop_sends(&fan_out, &["a"], &[]);
        let (_presence, mut helper) = fan_out.connect_helper(DESKTOP_1);
        helper.try_next();

        fan_out.request_force_release(DESKTOP_1);
        fan_out.request_force_release(DESKTOP_1);
        smbdrop_sends(&fan_out, &["b"], &[]);
        let update = helper.try_next().unwrap();
        assert!(update.force_release_obsolete);
        assert_eq!(helper_ids(&update), Some(vec!["b"]));
        assert_eq!(helper.try_next(), None);
    }

    #[test]
    fn the_mount_status_follows_desired_state_and_helpers() {
        let fan_out = fan_out();
        let mut statuses = fan_out.mount_status_feed();
        assert_eq!(
            statuses.try_next(),
            Some(None),
            "unknown before smbdrop told us"
        );

        smbdrop_sends(&fan_out, &["a"], &[]);
        assert_eq!(statuses.try_next(), Some(Some(AgentMountStatus::NoHelper)));

        let (presence, _feed) = fan_out.connect_helper(DESKTOP_1);
        assert_eq!(
            statuses.try_next(),
            Some(Some(AgentMountStatus::Reconciling))
        );

        presence.report(report(&["a"], &["a"]));
        assert_eq!(statuses.try_next(), Some(Some(AgentMountStatus::Idle)));
        assert_eq!(fan_out.mount_status(), Some(AgentMountStatus::Idle));

        // participants change, our status doesn't
        smbdrop_sends(&fan_out, &["a"], &[certified_at(2, "Holo", "detector")]);
        assert_eq!(statuses.try_next(), None);

        smbdrop_sends(&fan_out, &["a", "b"], &[]);
        assert_eq!(
            statuses.try_next(),
            Some(Some(AgentMountStatus::Reconciling))
        );

        drop(presence);
        assert_eq!(statuses.try_next(), Some(Some(AgentMountStatus::NoHelper)));
    }

    /// GUIs show our own status among the connected agents; it changes with any scope's helper
    #[test]
    fn guis_see_our_own_status_and_connection() {
        let fan_out = fan_out();
        let mut gui = fan_out.subscribe_gui(DESKTOP_1);
        gui.try_next();
        smbdrop_sends(&fan_out, &["a"], &[certified_at(1, "Holo", "k2")]);
        assert!(gui.try_next().unwrap().connected_agents.is_none());

        fan_out.logged_in(agent("Holo", "k2"));
        assert!(gui.try_next().is_none(), "not yet welcomed");
        fan_out.welcomed(ConnectionId(1));
        let own = gui.try_next().unwrap().connected_agents.unwrap().own;
        assert_eq!(own.status, Some(AgentMountStatus::NoHelper));

        let (_presence, _feed) = fan_out.connect_helper(DESKTOP_2);
        let own = gui.try_next().unwrap().connected_agents.unwrap().own;
        assert_eq!(own.status, Some(AgentMountStatus::Reconciling));

        fan_out.smbdrop_disconnected();
        assert!(gui.try_next().unwrap().connected_agents.is_none());
    }

    #[test]
    fn queries_combine_desired_state_and_the_scopes_report() {
        let fan_out = fan_out();
        assert_eq!(fan_out.desired_foreground(), None);
        assert_eq!(fan_out.mount_status(), None);

        let older = Share {
            dest_path: "/adhoc/project/older/".to_owned(),
            ..share("older")
        };
        let current = Share {
            dest_path: "/adhoc/project/current/".to_owned(),
            ..share("current")
        };
        fan_out.apply_share_list(vec![older, current.clone()], Some(current.clone()), vec![]);
        assert_eq!(fan_out.desired_foreground(), Some(current));
        assert!(fan_out.other_mounts_for(DESKTOP_1).is_empty());

        let (presence, _feed) = fan_out.connect_helper(DESKTOP_1);
        presence.report(report(&["older", "current"], &["current", "older"]));
        assert!(matches!(
            fan_out.foreground_for(DESKTOP_1),
            Ok(APICurrentForegroundShare::Mounted { .. })
        ));
        assert_eq!(
            fan_out
                .other_mounts_for(DESKTOP_1)
                .iter()
                .map(|mount| mount.session.as_str())
                .collect::<Vec<_>>(),
            vec!["older"]
        );
        // another scope has no helper
        assert!(fan_out.other_mounts_for(DESKTOP_2).is_empty());
        assert!(!matches!(
            fan_out.foreground_for(DESKTOP_2),
            Ok(APICurrentForegroundShare::Mounted { .. })
        ));
    }

    /// The async side: a waiting feed wakes up on a change
    #[tokio::test]
    async fn feeds_wait_for_changes() {
        let fan_out = fan_out();
        let (_presence, mut helper) = fan_out.connect_helper(DESKTOP_1);
        let mut gui = fan_out.subscribe_gui(DESKTOP_1);
        let mut statuses = fan_out.mount_status_feed();
        helper.next().await;
        gui.next().await;
        statuses.next().await;

        let waiting = tokio::spawn(async move {
            (
                helper_ids(&helper.next().await).map(|ids| ids.len()),
                gui.next().await.desired_shares.is_some(),
                statuses.next().await,
            )
        });
        tokio::task::yield_now().await;
        smbdrop_sends(&fan_out, &["a"], &[]);
        let seen = tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(seen, (Some(1), true, Some(AgentMountStatus::Reconciling)));
    }
}
