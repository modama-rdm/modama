use std::
    path::PathBuf
;

use modama_common::messages::Share;


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedShare {
    pub share: Share,

    /// The mount path; a drive letter on Windows, or a path on Linux
    pub mount_path: PathBuf,
}
