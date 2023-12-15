use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
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

/// Local system users can ask the agent for the current foreground share. This
/// doesn't have to contain credentials, most important is some kind of identity
/// and the path where the share is mounted.
#[derive(Serialize)]
#[serde(tag = "type")]
pub enum CurrentForegroundShare {
    None,
    Share {
        /// The path in the data management that is exported with this Share
        dest_path: String,

        /// Where the share is mounted on the local system
        mount_path: String,
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventChannelMessage {
    UpdateShareList {
        shares: Vec<ShareMsg>,
        foreground_share: Option<ShareMsg>,
    }
}