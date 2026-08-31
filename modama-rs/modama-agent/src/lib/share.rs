use std::path::PathBuf;

use modama_common::messages::Share;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountedShare {
    pub share: Share,

    /// The mount path; a drive letter on Windows, or a path on Linux
    pub mount_path: PathBuf,
}
