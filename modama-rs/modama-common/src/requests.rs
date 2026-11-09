use serde::{Deserialize, Serialize};

/// Ask smbdrop for a share of a folder. Name the folder with `canonical_path`; `dest_path`,
/// the full path under smbdrop's data root, is still accepted from older callers. Exactly one
/// of the two must be set.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateShareRequest {
    /// The canonical path of the folder to share, like `adhoc/testproject/session2`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_path: Option<String>,

    /// The folder to share as smbdrop sees it, data root included; for older callers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_path: Option<String>,

    pub resource: Option<String>,
    pub uid: u32,
    pub gid: u32,

    /// Login name of the user who asked for the share
    #[serde(default)]
    pub created_by: Option<String>,
}
