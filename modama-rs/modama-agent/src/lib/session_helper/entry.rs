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
// - [ ] mount backend WNet
// - [ ] open the file manager for newly created foreground mounts
// - [ ] notification tray icon

use anyhow::{Context as _, Result, anyhow};
use log::info;
use modama_common::logging::init_logging;
use url::Url;

use crate::{
    runtime::shutdown_signal,
    session_helper::{
        main_loop::{LoopOptions, session_helper_loop},
        platform::CurrentPlatformMountBackend,
        singleton::{self, Singleton},
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

/// Run the session helper until Ctrl-C/SIGTERM. Mounts stay in place when it exits.
pub fn run(agent_url: &Url) -> Result<()> {
    init_logging();

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
    tokio::runtime::Runtime::new()?.block_on(async {
        let backend = CurrentPlatformMountBackend::new().await?;
        tokio::select! {
            _ = session_helper_loop(&backend, &options) => {}
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
