//! The notification-area icon: shows whether the helper's mounts are in order, and offers to
//! open Modama and the current network drive.
//!
//! The icon only renders state and reports intent: menu clicks become [`TrayAction`]s that the
//! helper carries out, so no platform callback launches a process. The icon is optional in both
//! directions; without a notification area (no StatusNotifierHost, no Explorer shell), the
//! helper logs it once and runs without one.

use std::{path::PathBuf, process::Stdio};

use log::{info, warn};
use modama_common::{
    foreground::{ScopeView, project_and_session},
    internal_api_types::{SessionHelperState, ToSessionHelper},
};
use tokio::sync::{mpsc, watch};
use url::Url;

use crate::{mount_popup::FolderOpener as _, open_folder::SystemFolderOpener};

#[cfg(target_os = "linux")]
use crate::tray_linux as platform;
#[cfg(windows)]
use crate::tray_windows as platform;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TrayStatus {
    /// The helper can't reach the agent
    #[default]
    Disconnected,
    /// Actual state hasn't caught up with desired state yet; `reason` is the first error
    Working { reason: Option<String> },
    /// Actual state matches desired state
    Ready,
}

/// Everything the icon shows
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrayState {
    pub status: TrayStatus,
    /// Where the current foreground share is mounted, for "Open ingest share"
    pub network_drive: Option<PathBuf>,
    /// The session of the foreground share, while it is mounted
    pub foreground_session: Option<String>,
}

impl TrayState {
    /// The status line in the menu and the tooltip
    pub fn label(&self) -> String {
        match &self.status {
            TrayStatus::Disconnected => "Not connected to the agent".to_owned(),
            TrayStatus::Working { reason: None } => "Working…".to_owned(),
            TrayStatus::Working {
                reason: Some(reason),
            } => format!("Working: {reason}"),
            TrayStatus::Ready => match &self.foreground_session {
                Some(session) => format!("Ingest share for session {session}"),
                None => "Ready".to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenModama,
    OpenNetworkDrive,
}

/// What the icon should show, from what the helper knows: whether it is connected to the agent,
/// the last desired state, and its last pass's result
pub fn tray_state(
    connected: bool,
    desired: Option<&ToSessionHelper>,
    actual: Option<&SessionHelperState>,
) -> TrayState {
    // None: the agent hasn't heard from smbdrop yet, so whether we're done is unknown
    let view = desired
        .and_then(|desired| desired.share_state.as_ref())
        .map(|shares| ScopeView::new(Some(shares), actual));
    let network_drive = view
        .and_then(|view| view.foreground_mount_path())
        .map(PathBuf::from);
    let foreground_session = view
        .filter(|_| network_drive.is_some())
        .and_then(|view| view.foreground_share())
        .and_then(|share| project_and_session(&share.dest_path).ok())
        .map(|(_project, session)| session);
    let status = match (connected, view) {
        (false, _) => TrayStatus::Disconnected,
        (true, Some(view)) if !view.actual_differs_from_desired() => TrayStatus::Ready,
        (true, _) => TrayStatus::Working {
            reason: actual
                .and_then(|actual| actual.errors.first())
                .map(|error| error.message.clone()),
        },
    };
    TrayState {
        status,
        network_drive,
        foreground_session,
    }
}

/// The image for a `size` px icon from an `.ico` file: the smallest one at least that large, or
/// else the largest. (`LookupIconIdFromDirectoryEx` expects the resource layout, not the file's.)
#[cfg(any(windows, test))]
pub fn ico_image(ico: &[u8], size: u32) -> Option<&[u8]> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?));
    // ICONDIR: reserved, type (1 = icon), count; then 16-byte ICONDIRENTRYs
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return None;
    }
    let entries = (0..usize::from(u16_at(4)?)).map(|i| {
        let entry = 6 + 16 * i;
        // a width of 0 means 256
        let width = match *ico.get(entry)? {
            0 => 256,
            width => u32::from(width),
        };
        let length = usize::try_from(u32_at(entry + 8)?).ok()?;
        let offset = usize::try_from(u32_at(entry + 12)?).ok()?;
        Some((width, ico.get(offset..offset.checked_add(length)?)?))
    });
    let entries: Vec<_> = entries.collect::<Option<_>>()?;
    let fitting = entries
        .iter()
        .filter(|(width, _)| *width >= size)
        .min_by_key(|(width, _)| *width);
    let largest = entries.iter().max_by_key(|(width, _)| *width);
    fitting.or(largest).map(|(_, image)| *image)
}

/// Show the icon and keep it up to date with `state`; returns right away if there is no
/// notification area
pub async fn run_tray(
    mut state: watch::Receiver<TrayState>,
    actions: mpsc::UnboundedSender<TrayAction>,
) {
    let initial = state.borrow_and_update().clone();
    let item = match platform::start(initial, actions).await {
        Ok(item) => item,
        Err(e) => {
            info!("no notification-area icon: {e}");
            return;
        }
    };
    while state.changed().await.is_ok() {
        let current = state.borrow_and_update().clone();
        item.update(current).await;
    }
}

/// Start the GUI, which is our own executable. On X11, an already open GUI brings its window to
/// the front instead (`modama_agent_gui::single_instance`); elsewhere it gets a second window.
fn open_modama(agent_url: &Url) {
    let gui = match std::env::current_exe() {
        Ok(gui) => gui,
        Err(e) => {
            warn!("could not locate our own executable: {e}");
            return;
        }
    };
    let started = tokio::process::Command::new(&gui)
        .arg("--agent-url")
        .arg(agent_url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn();
    match started {
        // reaped in the background; the GUI may well outlive us
        Ok(mut child) => {
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
        }
        Err(e) => warn!("could not start {}: {e}", gui.display()),
    }
}

/// Carry out what the user chose in the icon's menu. Never returns: without an icon there is
/// nothing to wait for, but that doesn't end the helper.
pub async fn handle_tray_actions(
    mut actions: mpsc::UnboundedReceiver<TrayAction>,
    state: watch::Receiver<TrayState>,
    agent_url: &Url,
) {
    while let Some(action) = actions.recv().await {
        match action {
            TrayAction::OpenModama => open_modama(agent_url),
            TrayAction::OpenNetworkDrive => {
                let drive = state.borrow().network_drive.clone();
                if let Some(drive) = drive {
                    SystemFolderOpener.open(&drive).await;
                }
            }
        }
    }
    std::future::pending().await
}

#[cfg(test)]
mod tests {
    use modama_common::{
        internal_api_types::{MountedShare, SessionHelperConfig, ShareError, Shares},
        messages::Share,
    };

    use super::*;

    fn desired(shares: Vec<Share>, foreground: Option<&str>) -> ToSessionHelper {
        ToSessionHelper {
            config: SessionHelperConfig {
                smb_hostname: "smb.example.test".to_owned(),
                smb_port: 445,
                smb_domain: "WORKGROUP".to_owned(),
            },
            share_state: Some(Shares {
                foreground_share_id: foreground.map(str::to_owned),
                shares,
            }),
            force_release_obsolete: false,
        }
    }

    fn share(id: &str) -> Share {
        Share {
            resource: None,
            share_id: id.to_owned(),
            share_name: format!("share-{id}"),
            dest_path: format!("/project/{id}"),
            created_timestamp: 0,
            username: "user".to_owned(),
            password: "password".to_owned(),
            uid: 1000,
            gid: 1000,
            created_by: None,
        }
    }

    fn mounted(id: &str) -> SessionHelperState {
        SessionHelperState {
            mounted_shares: vec![MountedShare {
                share_name: format!("share-{id}"),
                mount_path: Some(format!("/mnt/{id}").into()),
            }],
            errors: vec![],
            reconciled_share_ids: None,
        }
    }

    #[test]
    fn ico_images_are_found_by_size() {
        let ico = include_bytes!("../assets/tray-icon.ico");
        let image = |size| ico_image(ico, size).map(<[u8]>::len);
        // the small sizes are 32-bit DIBs: a 40-byte header, the pixels, and a 1-bit mask
        assert_eq!(image(16), Some(40 + 16 * 16 * 4 + 16 * 4));
        assert_eq!(image(17), Some(40 + 20 * 20 * 4 + 20 * 4));
        // 256 is a PNG, which also serves sizes beyond it
        assert!(ico_image(ico, 256).unwrap().starts_with(b"\x89PNG"));
        assert!(ico_image(ico, 512).unwrap().starts_with(b"\x89PNG"));
        assert_eq!(ico_image(b"not an icon", 16), None);
    }

    #[test]
    fn disconnected_still_knows_the_network_drive() {
        let desired = desired(vec![share("a")], Some("a"));
        let state = tray_state(false, Some(&desired), Some(&mounted("a")));
        assert_eq!(state.status, TrayStatus::Disconnected);
        assert_eq!(state.network_drive, Some(PathBuf::from("/mnt/a")));
    }

    #[test]
    fn converged_names_the_foreground_share_s_session() {
        let desired = desired(vec![share("a")], Some("a"));
        let state = tray_state(true, Some(&desired), Some(&mounted("a")));
        assert_eq!(state.status, TrayStatus::Ready);
        assert_eq!(state.label(), "Ingest share for session a");
    }

    #[test]
    fn converged_without_an_ingest_share_is_ready() {
        let desired = desired(vec![], None);
        let nothing_mounted = SessionHelperState {
            mounted_shares: vec![],
            errors: vec![],
            reconciled_share_ids: None,
        };
        let state = tray_state(true, Some(&desired), Some(&nothing_mounted));
        assert_eq!(state.status, TrayStatus::Ready);
        assert_eq!(state.foreground_session, None);
        assert_eq!(state.label(), "Ready");
    }

    #[test]
    fn a_mount_reported_in_another_case_is_ready_and_openable() {
        let desired = desired(vec![share("a")], Some("a"));
        let mut actual = mounted("a");
        actual.mounted_shares[0].share_name = "SHARE-A".to_owned();
        let state = tray_state(true, Some(&desired), Some(&actual));
        assert_eq!(state.status, TrayStatus::Ready);
        assert_eq!(state.network_drive, Some(PathBuf::from("/mnt/a")));
    }

    #[test]
    fn working_until_converged_with_the_first_error_as_reason() {
        let desired = desired(vec![share("a"), share("b")], Some("b"));
        let mut actual = mounted("a");
        actual.errors.push(ShareError {
            share_id: "b".to_owned(),
            message: "another user's ingest share is still connected".to_owned(),
        });
        let state = tray_state(true, Some(&desired), Some(&actual));
        assert_eq!(
            state.label(),
            "Working: another user's ingest share is still connected"
        );
        assert_eq!(state.network_drive, None);

        // nothing known yet:
        assert_eq!(
            tray_state(true, None, None).status,
            TrayStatus::Working { reason: None }
        );
    }
}
