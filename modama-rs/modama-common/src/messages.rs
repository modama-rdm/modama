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

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum EventChannelInitialMessages {
    Login { token: String, version: String },
}

#[cfg(test)]
mod tests {
    use super::Share;
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
}
