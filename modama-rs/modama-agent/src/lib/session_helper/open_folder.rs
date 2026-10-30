//! Show a mounted share in the file manager, for the mount popup's "Open network drive".
//!
//! Windows: Explorer, started by its absolute path. Linux: the desktop's file manager through
//! `org.freedesktop.FileManager1`; `xdg-open` only if that interface definitely isn't there,
//! because `inode/directory` is often associated with an editor.

use std::path::Path;

use log::{debug, warn};

use crate::session_helper::mount_popup::FolderOpener;

pub struct SystemFolderOpener;

impl FolderOpener for SystemFolderOpener {
    async fn open(&self, path: &Path) {
        match open_folder(path).await {
            Ok(()) => debug!("opened {}", path.display()),
            Err(e) => warn!("could not open {} in the file manager: {e}", path.display()),
        }
    }
}

/// Start `program` without waiting for it; it is reaped in the background
fn spawn_detached(program: &Path, arg: &Path) -> Result<(), String> {
    let mut child = tokio::process::Command::new(program)
        .arg(arg)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

#[cfg(windows)]
async fn open_folder(path: &Path) -> Result<(), String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf};

    use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;

    let mut buffer = [0u16; 260];
    // SAFETY: the buffer is writable for its length; the call writes at most that many
    // characters and returns how many it wrote (or the size needed, if larger)
    // https://learn.microsoft.com/windows/win32/api/sysinfoapi/nf-sysinfoapi-getwindowsdirectoryw
    let length = unsafe { GetWindowsDirectoryW(Some(&mut buffer)) } as usize;
    if length == 0 || length > buffer.len() {
        return Err("could not find the Windows directory".to_owned());
    }
    let explorer = PathBuf::from(OsString::from_wide(&buffer[..length])).join("explorer.exe");
    spawn_detached(&explorer, path)
}

#[cfg(target_os = "linux")]
mod file_manager1 {
    use std::{path::Path, time::Duration};

    use url::Url;

    /// The file manager has this long to answer; a slow answer may still open a window
    const CALL_TIMEOUT: Duration = Duration::from_secs(10);

    pub enum ShowFoldersError {
        /// Definitely nothing was shown; another way may be tried
        Unavailable(String),
        /// Something went wrong, but a window may have opened anyway
        Failed(String),
    }

    /// Errors that mean nobody implements FileManager1 on this bus
    pub fn means_unavailable(error_name: &str) -> bool {
        matches!(
            error_name,
            "org.freedesktop.DBus.Error.ServiceUnknown"
                | "org.freedesktop.DBus.Error.NameHasNoOwner"
                | "org.freedesktop.DBus.Error.UnknownMethod"
                | "org.freedesktop.DBus.Error.UnknownInterface"
                | "org.freedesktop.DBus.Error.UnknownObject"
        )
    }

    pub async fn show_folder(path: &Path) -> Result<(), ShowFoldersError> {
        // the interface takes URIs; this one is only built for the call
        let uri = Url::from_directory_path(path)
            .map_err(|()| ShowFoldersError::Failed("not an absolute path".to_owned()))?;
        let connection = zbus::Connection::session()
            .await
            .map_err(|e| ShowFoldersError::Unavailable(format!("no session bus: {e}")))?;
        // no startup notification ID: we have no token to pass on
        let arguments = (vec![uri.as_str()], "");
        let call = connection.call_method(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            Some("org.freedesktop.FileManager1"),
            "ShowFolders",
            &arguments,
        );
        match tokio::time::timeout(CALL_TIMEOUT, call).await {
            Ok(Ok(_reply)) => Ok(()),
            Ok(Err(zbus::Error::MethodError(name, detail, _))) => {
                let message = format!("{name}: {}", detail.unwrap_or_default());
                if means_unavailable(name.as_str()) {
                    Err(ShowFoldersError::Unavailable(message))
                } else {
                    Err(ShowFoldersError::Failed(message))
                }
            }
            Ok(Err(e)) => Err(ShowFoldersError::Failed(e.to_string())),
            Err(_elapsed) => Err(ShowFoldersError::Failed("timed out".to_owned())),
        }
    }
}

#[cfg(target_os = "linux")]
async fn open_folder(path: &Path) -> Result<(), String> {
    use file_manager1::{ShowFoldersError, show_folder};

    match show_folder(path).await {
        Ok(()) => Ok(()),
        Err(ShowFoldersError::Failed(e)) => Err(e),
        Err(ShowFoldersError::Unavailable(e)) => {
            debug!("FileManager1 is not available ({e}), using xdg-open");
            spawn_detached(Path::new("xdg-open"), path)
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::file_manager1::means_unavailable;

    #[test]
    fn only_a_missing_file_manager_falls_back() {
        assert!(means_unavailable(
            "org.freedesktop.DBus.Error.ServiceUnknown"
        ));
        assert!(means_unavailable(
            "org.freedesktop.DBus.Error.UnknownMethod"
        ));
        assert!(!means_unavailable("org.freedesktop.DBus.Error.NoReply"));
        assert!(!means_unavailable("org.freedesktop.DBus.Error.Failed"));
    }
}
