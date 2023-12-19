use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::messages::ShareMsg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// A UUID for this share
    pub share_id: String,

    /// The full share name; the first path element of the SMB URL
    pub share_name: String,

    /// The path in the data management that is exported with this Share
    pub dest_path: String,

    /// When was this share created?
    pub created_timestamp: SystemTime,

    /// Username for connecting (temporary)
    pub username: String,

    /// Password for connecting (temporary)
    pub password: String,
}

impl From<ShareMsg> for Share {
    fn from(value: ShareMsg) -> Self {
        Self {
            share_id: value.share_id,
            share_name: value.share_name,
            dest_path: value.dest_path,
            created_timestamp: UNIX_EPOCH + Duration::from_secs(value.created_timestamp),
            username: value.username,
            password: value.password,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedShare {
    pub share: Share,

    /// The mount path; a drive letter on Windows, or a path on Linux
    pub mount_path: PathBuf,
}
