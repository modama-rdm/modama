//! Wire format used in the internal websocket API between modama-agent, the session helper and the
//! GUI. Don't put non-wire types here.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHelperConfig {
    pub smb_hostname: String,
    pub smb_port: u32,
    pub smb_domain: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Shares {
    /// The foreground share ID is needed to have the notification tray UI present it
    pub foreground_share_id: Option<String>,

    /// All shares that should be mounted
    pub shares: Vec<crate::messages::Share>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToSessionHelper {
    /// Configuration contains the samba hostname whose mounts are managed by the session helper
    /// (also the port if we are on Linux)
    pub config: SessionHelperConfig,

    /// None means we haven't received the desired state yet. We need to consider this, because
    /// otherwise, it could lead to a flip-flopping situation when re-connecting. If this is
    /// None, and shares are mounted, no reconciliation should be happening.
    pub share_state: Option<Shares>,

    /// Set only on the one message the agent sends when the GUI of this scope asked to force the
    /// release: the pass it triggers removes mounts that are no longer desired, even if files
    /// are open on them. Not a lasting mode; later messages don't carry it.
    #[serde(default)]
    pub force_release_obsolete: bool,
}

/// What the GUI may send to the agent
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToAgentFromGui {
    /// Force the release of mounts that are no longer desired (another user's drive that blocks
    /// the hand-off), in the GUI's own scope
    ForceRelease,
}

/// Desired state and the session helper's report, for the GUI's own scope. The agent determines
/// the scope from the connection, so it isn't part of the message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToSessionGui {
    /// The last report of the session helper; None if we haven't received any yet
    /// (which means shares could be mounted already, but we don't know about it)
    pub session_helper: Option<SessionHelperState>,

    /// The currently desired shares, as received from smbdrop; None if we
    /// haven't received any shares yet (meaning: no reconciliation should be
    /// happening in that case!)
    pub desired_shares: Option<Shares>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ToAgent {
    /// Update from the session helper informing about the currently mounted ingest shares
    SessionUpdate {
        shares: Vec<MountedShare>,
        errors: Vec<ShareError>,
    },
}

/// What the session helper reported last: mounts and per-share errors are kept together, so
/// they are either both known or both unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionHelperState {
    pub mounted_shares: Vec<MountedShare>,
    pub errors: Vec<ShareError>,
}

/// Why a desired share is not mounted
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareError {
    /// errors are about desired shares, so unlike `MountedShare`, we can identify them by ID
    pub share_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountedShare {
    /// there can be shares mounted that we don't have any information about
    /// except that they do belong to the ingest samba server, their share name, and where they are
    /// mounted (or not, in case of non-drive-mapped share use!) - so we can't use `Share` here,
    /// and have to identify shares by their name.
    pub share_name: String,

    /// The mount path; a drive letter on Windows, or a path on Linux. None if there is no
    /// mount point (broken gvfs on Linux, non-mapped access on Windows)
    pub mount_path: Option<PathBuf>,
}
