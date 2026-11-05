//! What smbdrop's event channel tells agents about: the shares, and the open connections with
//! their mount status (see ADR 0001: smbdrop tracks connections, not identities). Everything
//! lives in one value behind one lock (the `watch` channel's); every change notifies the
//! subscribers, which render the update for their resource from the current value. A subscriber
//! that falls behind skips intermediate values, but never the latest one. A change and its
//! notification are one step, so callers can't get their order wrong.

use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
};

use log::info;
use modama_common::messages::{
    AgentMountStatus, ConnectionId, EventChannelMessage, Participant, ParticipantIdentity, Share,
};
use tokio::sync::watch;

/// The state of the event channel; see the module documentation
#[derive(Debug, Clone)]
pub struct EventHub {
    state: watch::Sender<HubState>,
}

/// One open event channel connection
#[derive(Debug)]
struct Connection {
    identity: ParticipantIdentity,

    peer: SocketAddr,

    /// The last mount status the agent reported on this connection
    status: Option<AgentMountStatus>,
}

#[derive(Debug)]
struct HubState {
    /// All shares, by share ID, as the HTTP API set them last
    shares: HashMap<String, Share>,

    /// The open connections, in the order they were registered
    connections: BTreeMap<ConnectionId, Connection>,

    /// The number for the next connection
    next_connection: u64,
}

impl HubState {
    /// The update for the connections at `resource`: its shares, sorted by creation time with
    /// the newest as the foreground share, and its participants. Shares created at the same
    /// time are sorted by ID, so that the same state always renders the same update.
    fn update_for(&self, resource: &str) -> EventChannelMessage {
        let mut shares: Vec<Share> = self
            .shares
            .values()
            .filter(|share| share.resource.as_deref() == Some(resource))
            .cloned()
            .collect();
        shares.sort_by(|a, b| {
            (a.created_timestamp, &a.share_id).cmp(&(b.created_timestamp, &b.share_id))
        });
        // only those at the receiver's resource: other resources are none of its business
        let participants = self
            .connections
            .iter()
            .filter(|(_, connection)| connection.identity.resource() == resource)
            .map(|(id, connection)| Participant {
                connection: *id,
                identity: connection.identity.clone(),
                peer: connection.peer,
                status: connection.status,
            })
            .collect();
        EventChannelMessage::UpdateShareList {
            foreground_share: shares.last().cloned(),
            shares,
            participants,
        }
    }
}

impl EventHub {
    /// A hub with `shares` (by share ID) and no connections
    pub fn new(shares: HashMap<String, Share>) -> Self {
        let (state, _) = watch::channel(HubState {
            shares,
            connections: BTreeMap::new(),
            next_connection: 0,
        });
        Self { state }
    }

    /// Replace all shares with `shares` (by share ID)
    pub fn set_shares(&self, shares: HashMap<String, Share>) {
        self.state.send_if_modified(|state| {
            if state.shares == shares {
                return false;
            }
            state.shares = shares;
            true
        });
    }

    /// List a new connection with `identity` from `peer`, until the returned registration is
    /// dropped
    pub fn register(
        &self,
        identity: ParticipantIdentity,
        peer: SocketAddr,
    ) -> ConnectionRegistration {
        let mut id = ConnectionId(0);
        self.state.send_modify(|state| {
            id = ConnectionId(state.next_connection);
            state.next_connection = state.next_connection.wrapping_add(1);
            state.connections.insert(
                id,
                Connection {
                    identity,
                    peer,
                    status: None,
                },
            );
        });
        ConnectionRegistration {
            hub: self.clone(),
            id,
        }
    }

    /// The updates for the connections at `resource`
    pub fn subscribe(&self, resource: &str) -> ResourceUpdates {
        ResourceUpdates {
            changes: self.state.subscribe(),
            resource: resource.to_owned(),
            sent: None,
        }
    }
}

/// A connection's entry in the hub, for as long as this value lives: it removes the entry when
/// dropped, whichever way the connection handler ends (return, error or cancellation)
#[derive(Debug)]
pub struct ConnectionRegistration {
    hub: EventHub,
    id: ConnectionId,
}

impl ConnectionRegistration {
    pub fn id(&self) -> ConnectionId {
        self.id
    }

    /// The agent on this connection reported `status`. If it differs from the previous status,
    /// the connections at the same resource get an update.
    pub fn report_status(&self, status: AgentMountStatus) {
        self.hub.state.send_if_modified(|state| {
            match state.connections.get_mut(&self.id) {
                Some(connection) if connection.status != Some(status) => {
                    connection.status = Some(status);
                    true
                }
                // unchanged; or the connection has no entry, which can't happen while this
                // registration exists
                Some(_) | None => false,
            }
        });
    }
}

impl Drop for ConnectionRegistration {
    fn drop(&mut self) {
        self.hub.state.send_modify(|state| {
            state.connections.remove(&self.id);
        });
        info!("connection {} closed", self.id);
    }
}

/// The updates for the connections at one resource, without repeats: a change elsewhere, or
/// one that leaves the update as it was, brings no new update
#[derive(Debug)]
pub struct ResourceUpdates {
    changes: watch::Receiver<HubState>,
    resource: String,

    /// None until the first update
    sent: Option<EventChannelMessage>,
}

impl ResourceUpdates {
    /// The current update, whether it was sent before or not
    pub fn current(&mut self) -> EventChannelMessage {
        let update = self.changes.borrow_and_update().update_for(&self.resource);
        self.sent = Some(update.clone());
        update
    }

    /// The current update, if it differs from the last one returned; the first call returns it
    /// in any case
    pub fn try_next(&mut self) -> Option<EventChannelMessage> {
        let update = self.changes.borrow_and_update().update_for(&self.resource);
        if self.sent.as_ref() == Some(&update) {
            return None;
        }
        self.sent = Some(update.clone());
        Some(update)
    }

    /// Wait for the next update that differs from the last one; cancel-safe
    pub async fn next(&mut self) -> EventChannelMessage {
        loop {
            if let Some(update) = self.try_next() {
                return update;
            }
            // the sender lives as long as any clone of its `EventHub`; without one, there is no
            // change to wait for
            if self.changes.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, net::SocketAddr, time::Duration};

    use modama_common::{
        agent::AgentId,
        messages::{
            AgentMountStatus, EventChannelMessage, Participant, ParticipantIdentity, Share,
        },
    };
    use tokio::time::timeout;

    use super::{EventHub, ResourceUpdates};

    fn share(id: &str, resource: &str, created_timestamp: u64) -> Share {
        Share {
            resource: Some(resource.to_owned()),
            share_id: id.to_owned(),
            share_name: format!("share-{id}"),
            dest_path: format!("/data/dest/{id}"),
            created_timestamp,
            username: "u".to_owned(),
            password: "p".to_owned(),
            uid: 1000,
            gid: 1000,
            created_by: None,
        }
    }

    fn shares(list: &[Share]) -> HashMap<String, Share> {
        list.iter()
            .map(|share| (share.share_id.clone(), share.clone()))
            .collect()
    }

    fn certified(resource: &str, agent: &str) -> ParticipantIdentity {
        ParticipantIdentity::Certified {
            agent: AgentId {
                resource: resource.to_owned(),
                agent: agent.to_owned(),
                san_encoded: format!("x-modama-agent://{resource}#{agent}"),
            },
        }
    }

    fn peer(n: u8) -> SocketAddr {
        format!("192.0.2.{n}:50000").parse().unwrap()
    }

    /// The share IDs and the participants' agent names and status of `update`
    type Summary = (Vec<String>, Vec<(String, Option<AgentMountStatus>)>);

    fn summary(update: &EventChannelMessage) -> Summary {
        let EventChannelMessage::UpdateShareList {
            shares,
            participants,
            ..
        } = update
        else {
            panic!("expected an update, got {update:?}");
        };
        let name = |participant: &Participant| match &participant.identity {
            ParticipantIdentity::Certified { agent } => agent.agent.clone(),
            ParticipantIdentity::Anonymous { name, .. } => name.clone(),
        };
        (
            shares.iter().map(|share| share.share_id.clone()).collect(),
            participants
                .iter()
                .map(|participant| (name(participant), participant.status))
                .collect(),
        )
    }

    fn expect(shares: &[&str], participants: &[(&str, Option<AgentMountStatus>)]) -> Summary {
        (
            shares.iter().map(|id| (*id).to_owned()).collect(),
            participants
                .iter()
                .map(|(name, status)| ((*name).to_owned(), *status))
                .collect(),
        )
    }

    /// The next update, which must come right away
    async fn next(updates: &mut ResourceUpdates) -> Summary {
        summary(
            &timeout(Duration::from_millis(500), updates.next())
                .await
                .unwrap(),
        )
    }

    fn assert_no_update(updates: &mut ResourceUpdates) {
        let update = updates.try_next();
        assert!(update.is_none(), "unexpected update: {update:?}");
    }

    #[tokio::test]
    async fn first_update_is_the_current_state() {
        let hub = EventHub::new(shares(&[share("a", "here", 0)]));
        let _registration = hub.register(certified("here", "agent"), peer(1));
        let mut updates = hub.subscribe("here");
        assert_eq!(next(&mut updates).await, expect(&["a"], &[("agent", None)]));
        assert_no_update(&mut updates);
    }

    /// When a connection comes after a share update, those already connected get the new shares
    /// with the new connection, and never the shares from before
    #[tokio::test]
    async fn a_connection_between_two_share_updates_never_brings_back_older_shares() {
        let hub = EventHub::new(shares(&[share("old", "here", 0)]));
        let _first = hub.register(certified("here", "first"), peer(1));
        let mut first_updates = hub.subscribe("here");
        assert_eq!(
            next(&mut first_updates).await,
            expect(&["old"], &[("first", None)])
        );

        hub.set_shares(shares(&[share("new", "here", 1)]));
        let _second = hub.register(certified("here", "second"), peer(2));
        let mut second_updates = hub.subscribe("here");

        let both = expect(&["new"], &[("first", None), ("second", None)]);
        assert_eq!(next(&mut first_updates).await, both);
        assert_eq!(next(&mut second_updates).await, both);
        assert_no_update(&mut first_updates);
        assert_no_update(&mut second_updates);
    }

    /// A subscriber that didn't keep up with the changes gets the latest state, once
    #[tokio::test]
    async fn a_slow_subscriber_gets_the_latest_state() {
        let hub = EventHub::new(HashMap::new());
        let mut updates = hub.subscribe("here");
        next(&mut updates).await;

        let mut registrations = vec![];
        for n in 0..100u64 {
            let id = n.to_string();
            hub.set_shares(shares(&[share(&id, "here", n)]));
            registrations.push(hub.register(certified("here", &id), peer(1)));
        }
        let (share_ids, participants) = next(&mut updates).await;
        assert_eq!(share_ids, vec!["99".to_owned()]);
        assert_eq!(participants.len(), 100);
        assert_no_update(&mut updates);
    }

    /// Each resource sees its own shares and participants, and only changes to those
    #[tokio::test]
    async fn updates_are_rendered_per_resource() {
        let hub = EventHub::new(shares(&[share("a", "here", 0), share("b", "there", 0)]));
        let _here = hub.register(certified("here", "near"), peer(1));
        let _there = hub.register(certified("there", "far"), peer(2));
        let mut here = hub.subscribe("here");
        let mut there = hub.subscribe("there");
        assert_eq!(next(&mut here).await, expect(&["a"], &[("near", None)]));
        assert_eq!(next(&mut there).await, expect(&["b"], &[("far", None)]));

        // changes at one resource don't reach the other
        let _other = hub.register(certified("there", "farther"), peer(3));
        hub.set_shares(shares(&[
            share("a", "here", 0),
            share("b", "there", 0),
            share("c", "there", 1),
        ]));
        assert_eq!(
            next(&mut there).await,
            expect(&["b", "c"], &[("far", None), ("farther", None)])
        );
        assert_no_update(&mut here);
    }

    /// The newest share is the foreground share; shares created at the same time are sorted by
    /// ID
    #[tokio::test]
    async fn the_newest_share_is_in_the_foreground() {
        let hub = EventHub::new(shares(&[
            share("z", "here", 0),
            share("y", "here", 2),
            share("x", "here", 2),
        ]));
        let mut updates = hub.subscribe("here");
        let update = timeout(Duration::from_millis(500), updates.next())
            .await
            .unwrap();
        assert_eq!(summary(&update).0, vec!["z", "x", "y"]);
        let EventChannelMessage::UpdateShareList {
            foreground_share, ..
        } = update
        else {
            panic!("expected an update");
        };
        assert_eq!(foreground_share.unwrap().share_id, "y");
    }

    /// A reported status is listed with its connection; reporting it again changes nothing
    #[tokio::test]
    async fn a_changed_status_is_an_update() {
        let hub = EventHub::new(HashMap::new());
        let registration = hub.register(certified("here", "agent"), peer(1));
        let mut updates = hub.subscribe("here");
        next(&mut updates).await;

        registration.report_status(AgentMountStatus::Idle);
        assert_eq!(
            next(&mut updates).await,
            expect(&[], &[("agent", Some(AgentMountStatus::Idle))])
        );
        registration.report_status(AgentMountStatus::Idle);
        assert_no_update(&mut updates);
    }

    /// Setting the same shares again is no update
    #[tokio::test]
    async fn the_same_shares_are_no_update() {
        let hub = EventHub::new(shares(&[share("a", "here", 0)]));
        let mut updates = hub.subscribe("here");
        next(&mut updates).await;
        hub.set_shares(shares(&[share("a", "here", 0)]));
        assert_no_update(&mut updates);
    }

    /// A connection is listed until its registration is dropped, with its status; the others
    /// stay, and each connection has its own ID
    #[tokio::test]
    async fn a_dropped_registration_is_no_longer_listed() {
        let hub = EventHub::new(HashMap::new());
        let first = hub.register(certified("here", "agent"), peer(1));
        let second = hub.register(certified("here", "agent"), peer(1));
        assert_ne!(first.id(), second.id());
        first.report_status(AgentMountStatus::Idle);
        second.report_status(AgentMountStatus::NoHelper);
        let mut updates = hub.subscribe("here");
        assert_eq!(
            next(&mut updates).await,
            expect(
                &[],
                &[
                    ("agent", Some(AgentMountStatus::Idle)),
                    ("agent", Some(AgentMountStatus::NoHelper))
                ]
            )
        );

        drop(first);
        assert_eq!(
            next(&mut updates).await,
            expect(&[], &[("agent", Some(AgentMountStatus::NoHelper))])
        );
    }

    /// An explicitly requested update comes even when it is unchanged, and isn't sent again
    #[tokio::test]
    async fn the_current_update_comes_on_request() {
        let hub = EventHub::new(HashMap::new());
        let mut updates = hub.subscribe("here");
        let first = updates.current();
        assert_eq!(updates.current(), first);
        assert_no_update(&mut updates);
    }
}
