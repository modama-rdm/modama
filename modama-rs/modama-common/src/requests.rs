use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateShareRequest {
    pub dest_path: String,
    pub resource: Option<String>,
    pub uid: u32,
    pub gid: u32,

    /// Login name of the user who asked for the share
    #[serde(default)]
    pub created_by: Option<String>,
}
