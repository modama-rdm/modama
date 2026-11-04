use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::agent::AgentId;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, ToSchema)]
pub struct Share {
    /// the name of the Resource this share belongs to; None if it wasn't
    /// created for a Resource
    pub resource: Option<String>,

    /// A UUID for this share
    pub share_id: String,

    /// The full share name; the first path element of the SMB URL
    pub share_name: String,

    /// The path in the data management that is exported with this Share
    pub dest_path: String,

    /// When was this share created? Unix timestamp
    pub created_timestamp: u64,

    /// Username for connecting (temporary)
    pub username: String,

    /// Password for connecting (temporary)
    pub password: String,

    /// the underlying unix user for which the share is created
    pub uid: u32,

    /// the underlying unix group for which the share is created
    pub gid: u32,

    /// Login name of the user who created the share; None for shares created before smbdrop
    /// recorded it
    #[serde(default)]
    pub created_by: Option<String>,
}

impl Share {
    pub fn get_created_systime(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.created_timestamp)
    }

    pub fn get_dest_path(&self) -> PathBuf {
        PathBuf::from(&self.dest_path)
    }
}

impl std::fmt::Debug for Share {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Share")
            .field("resource", &self.resource)
            .field("share_id", &self.share_id)
            .field("share_name", &self.share_name)
            .field("dest_path", &self.dest_path)
            .field("created_timestamp", &self.created_timestamp)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .field("created_by", &self.created_by)
            .finish()
    }
}

/// Version of the event channel protocol between smbdrop and the agents.
///
/// - 0: `UpdateShareList` and `ShareListUpdate` only; no `Welcome`
/// - 1: `Welcome`, `ReportMountStatus`, `agent_mount_status` in `UpdateShareList`
///
/// Both sides use the minimum of their own version and the peer's: the agent sends its
/// version in `Login`, smbdrop answers with `Welcome` only to agents that sent 1 or more,
/// before any `UpdateShareList`. An agent that gets an `UpdateShareList` first talks to
/// version 0.
///
/// Rules for changes: a side sends a new message or request variant only if the negotiated
/// version supports it; new fields must be optional (`#[serde(default)]`), so older peers
/// can leave them out and ignore them.
pub const EVENT_CHANNEL_PROTOCOL: u32 = 1;

/// The mount status of an agent, summarized over all of its session helpers
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentMountStatus {
    /// Every session helper has reconciled the current desired state, and actual matches desired
    Idle,

    /// A session helper hasn't reported for the current desired state yet
    Reconciling,

    /// Every session helper has reconciled the current desired state, but at least one couldn't
    /// make actual match desired, for example a mount that can't be closed, or a share that
    /// can't be mounted. Passes are retried periodically.
    LeftoverWork,

    /// No session helper is connected to the agent, so nothing is mounted for anybody
    NoHelper,

    /// A status a newer peer sent that this version doesn't know
    #[serde(other)]
    Unknown,
}

/// One connection to smbdrop's event channel, numbered by smbdrop as it accepts them; unique
/// while that smbdrop runs (the counter wraps after 2^64 connections)
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct ConnectionId(pub u64);

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// The last mount status an agent reported
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentMountStatusEntry {
    pub agent: AgentId,
    pub status: AgentMountStatus,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
#[allow(
    clippy::large_enum_variant,
    reason = "deserialized once per message and handled right away; boxing only adds noise"
)]
pub enum EventChannelMessage {
    UpdateShareList {
        shares: Vec<Share>,
        foreground_share: Option<Share>,
        connected_agents: Vec<AgentId>,

        /// Mount status of the connected agents that reported one; agents that didn't
        /// (yet) report are missing. Protocol 1 and up.
        #[serde(default)]
        agent_mount_status: Vec<AgentMountStatusEntry>,
    },

    /// First message to agents that logged in with protocol 1 or more
    Welcome { protocol: u32 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum EventChannelRequests {
    ShareListUpdate,

    /// The agent's mount status changed. Protocol 1 and up.
    ReportMountStatus {
        status: AgentMountStatus,
    },
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum EventChannelInitialMessages {
    Login {
        token: String,
        version: String,

        /// The highest event channel protocol version the agent speaks; agents from before
        /// protocol versions existed leave it out, which means 0
        #[serde(default)]
        protocol: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::{AgentMountStatus, EventChannelInitialMessages, EventChannelMessage, Share};
    use crate::requests::CreateShareRequest;

    /// Shares persisted by an smbdrop that didn't record the creator yet
    #[test]
    fn share_without_created_by_deserializes() {
        let share: Share = serde_json::from_value(serde_json::json!({
            "resource": "holo",
            "share_id": "b4fdcdf6-e870-4ac5-9942-78463f051c5c",
            "share_name": "share-b4fdcdf6",
            "dest_path": "/adhoc/project/session",
            "created_timestamp": 0,
            "username": "shareb4fdcdf6",
            "password": "secret",
            "uid": 1001,
            "gid": 1001,
        }))
        .unwrap();
        assert_eq!(share.created_by, None);
    }

    /// Requests from callers that don't send the creator yet
    #[test]
    fn create_request_without_created_by_deserializes() {
        let request: CreateShareRequest = serde_json::from_value(serde_json::json!({
            "dest_path": "/adhoc/project/session",
            "resource": null,
            "uid": 1001,
            "gid": 1001,
        }))
        .unwrap();
        assert_eq!(request.created_by, None);
    }

    /// Login from an agent from before protocol versions
    #[test]
    fn login_without_protocol_is_protocol_0() {
        let login: EventChannelInitialMessages = serde_json::from_value(serde_json::json!({
            "type": "Login",
            "token": "t",
            "version": "v",
        }))
        .unwrap();
        let EventChannelInitialMessages::Login { protocol, .. } = login;
        assert_eq!(protocol, 0);
    }

    /// Update from an smbdrop that speaks protocol 0
    #[test]
    fn update_without_mount_status_deserializes() {
        let msg: EventChannelMessage = serde_json::from_value(serde_json::json!({
            "type": "UpdateShareList",
            "shares": [],
            "foreground_share": null,
            "connected_agents": [],
        }))
        .unwrap();
        assert_eq!(
            msg,
            EventChannelMessage::UpdateShareList {
                shares: vec![],
                foreground_share: None,
                connected_agents: vec![],
                agent_mount_status: vec![],
            }
        );
    }

    /// A status added by a later version doesn't break the whole update
    #[test]
    fn unknown_mount_status_is_unknown() {
        let status: AgentMountStatus = serde_json::from_str("\"defragmenting\"").unwrap();
        assert_eq!(status, AgentMountStatus::Unknown);
        let status: AgentMountStatus = serde_json::from_str("\"leftover_work\"").unwrap();
        assert_eq!(status, AgentMountStatus::LeftoverWork);
    }
}
