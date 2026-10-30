//! The mount popup's protocol: the session helper starts the GUI executable with
//! `--mount-popup` and talks to it in JSON lines over the child's stdin/stdout.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Helper → popup (stdin): everything the popup should show, the full list each time
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopupContent {
    pub mounts: Vec<PopupMount>,
}

/// A share the helper has just mounted
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopupMount {
    pub project: String,
    pub session: String,
    /// The drive root (Windows) or local mount path (Linux)
    pub mount_path: PathBuf,
}

/// Popup → helper (stdout): the user's answer, one line; the popup exits afterwards. A popup
/// that exits without answering (closed window, no display) counts as `Dismiss`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopupChoice {
    Open,
    Dismiss,
}
