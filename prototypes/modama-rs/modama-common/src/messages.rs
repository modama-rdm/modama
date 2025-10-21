use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::agent::AgentId;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
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
            .finish()
    }
}

/// Local system users can ask the agent for the current foreground share. This
/// doesn't have to contain credentials, most important is some kind of identity
/// and the path where the share is mounted.
#[derive(Serialize, Clone)]
#[serde(tag = "type")]
pub enum CurrentForegroundShare {
    None,
    Share {
        /// The path in the data management that is exported with this Share
        dest_path: String,

        /// Where the share is mounted on the local system
        mount_path: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum EventChannelMessage {
    UpdateShareList {
        shares: Vec<Share>,
        foreground_share: Option<Share>,
        connected_agents: Vec<AgentId>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum EventChannelRequests {
    ShareListUpdate,
}
