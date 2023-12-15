use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::share::Share;

#[derive(Serialize, Deserialize, Clone)]
pub struct ShareMsg {
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
}

impl From<Share> for ShareMsg {
    fn from(value: Share) -> Self {
        Self {
            share_id: value.share_id,
            share_name: value.share_name,
            dest_path: value.dest_path,
            created_timestamp: value.created_timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs(),
            username: value.username,
            password: value.password,
        }
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

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum EventChannelMessage {
    UpdateShareList {
        shares: Vec<ShareMsg>,
        foreground_share: Option<ShareMsg>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum EventChannelRequests {
    ShareListUpdate,
}
