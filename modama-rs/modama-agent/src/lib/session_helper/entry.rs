// startup sequence:
//
// - refuse to run elevated (Windows; split UAC tokens only)
// - check singleton "lock"/"mutex" and exit if another session helper is running
// - connect to the local internal websocket API and run the reconciliation loop
//
//
// implementation TODO:
//
// - [x] singleton
// - [x] connection and reconciliation loop
// - [x] mount backend GIO
// - [x] mount backend WNet
// - [x] mount popup for newly created mounts, opening the file manager on request
// - [x] notification tray icon

use anyhow::{Context as _, Result, anyhow};
use log::info;
#[cfg(not(windows))]
use modama_common::logging::init_logging;
use tokio::sync::{mpsc, watch};
use url::Url;

use crate::{
    runtime::shutdown_signal,
    session_helper::{
        main_loop::{LoopOptions, session_helper_loop},
        mount_popup::{GuiPopups, run_popup_task},
        open_folder::SystemFolderOpener,
        platform::CurrentPlatformMountBackend,
        singleton::{self, Singleton},
        tray::{TrayState, handle_tray_actions, run_tray},
    },
};

/// The session helper endpoint, derived from the agent's base URL (`http://localhost:3003/`)
fn events_url(agent_url: &Url) -> Result<Url> {
    let mut url = agent_url
        .join("internal/events/session-helper/")
        .context("invalid agent URL")?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => return Err(anyhow!("unsupported agent URL scheme {other:?}")),
    };
    url.set_scheme(scheme)
        .map_err(|()| anyhow!("could not derive the websocket URL from {agent_url}"))?;
    Ok(url)
}

/// On Windows, the helper usually runs without a console (started by the GUI or at logon), so
/// it logs into a file in the user's local app data. Two sessions of one user (console and RDP)
/// share that file.
#[cfg(windows)]
fn init_helper_logging() -> Result<()> {
    use windows::Win32::UI::Shell::CSIDL_LOCAL_APPDATA;

    let log_dir = crate::windows_service::known_folder(CSIDL_LOCAL_APPDATA)?
        .join("modama")
        .join("modama-agent")
        .join("logs");
    eprintln!(
        "logging to {}",
        log_dir.join("session-helper.log").display()
    );
    crate::windows_service::init_file_logging(&log_dir, "session-helper.log")
}

#[cfg(not(windows))]
fn init_helper_logging() -> Result<()> {
    init_logging();
    Ok(())
}

/// Run the session helper until Ctrl-C/SIGTERM. Mounts stay in place when it exits.
pub fn run(agent_url: &Url) -> Result<()> {
    init_helper_logging()?;

    #[cfg(windows)]
    if singleton::is_elevated().context("could not check whether we run elevated")? {
        return Err(anyhow!(
            "the session helper must not run elevated: its drive mappings would not be visible \
             to Explorer; start it without 'Run as administrator'"
        ));
    }

    let _guard = match singleton::acquire()? {
        Singleton::Acquired(guard) => guard,
        Singleton::AlreadyRunning => {
            info!("a session helper is already running, exiting");
            return Ok(());
        }
    };

    let options = LoopOptions::new(events_url(agent_url)?);
    let popups = GuiPopups::next_to_current_exe().context("could not locate the GUI")?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let backend = CurrentPlatformMountBackend::new().await?;
        let (popup_tx, popup_rx) = mpsc::unbounded_channel();
        let (tray_tx, tray_rx) = watch::channel(TrayState::default());
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        tokio::select! {
            _ = session_helper_loop(&backend, &options, popup_tx, tray_tx) => {}
            () = run_popup_task(popup_rx, popups, SystemFolderOpener) => {}
            () = async {
                // without a notification area, the helper carries on without an icon
                run_tray(tray_rx.clone(), action_tx).await;
                std::future::pending().await
            } => {}
            () = handle_tray_actions(action_rx, tray_rx.clone(), agent_url) => {}
            () = shutdown_signal() => info!("shutting down"),
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_url_is_derived_from_the_agent_url() {
        let url = |s: &str| events_url(&Url::parse(s).unwrap()).map(|u| u.to_string());
        assert_eq!(
            url("http://localhost:3003/").unwrap(),
            "ws://localhost:3003/internal/events/session-helper/"
        );
        assert_eq!(
            url("https://localhost:3003").unwrap(),
            "wss://localhost:3003/internal/events/session-helper/"
        );
        assert!(url("ftp://localhost/").is_err());
    }
}
