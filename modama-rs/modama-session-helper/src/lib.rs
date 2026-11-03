//! The session helper: mounts the ingest shares of the agent's desired state in the user's
//! session (WNet drive mappings on Windows, GVfs on Linux), and shows the mount popup and the
//! notification-area icon. It runs from the GUI executable (`modama-agent-gui session-helper`).

#[cfg(windows)]
mod dpi_windows;
pub mod entry;
#[cfg(target_os = "linux")]
pub mod gio_backend;
pub mod main_loop;
pub mod mount_backend;
pub mod mount_popup;
pub mod open_folder;
#[cfg_attr(target_os = "linux", path = "platform_linux.rs")]
#[cfg_attr(target_os = "windows", path = "platform_windows.rs")]
pub mod platform;
pub mod singleton;
pub mod tray;
#[cfg(target_os = "linux")]
mod tray_linux;
#[cfg(windows)]
mod tray_windows;
#[cfg(any(windows, test))]
pub mod wnet_reconcile;
