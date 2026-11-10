// No console window on Windows: the GUI and the always-running session helper are both started
// from this executable. Debug builds keep the console for their log output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{error::Error, path::PathBuf, sync::Arc};

use clap::{CommandFactory, Parser, Subcommand};
use eframe::egui;
use modama_agent_gui::{
    app::AgentGuiApp,
    config::load_config,
    mount_popup,
    service::HttpAgentService,
    single_instance::{self, Claim},
    styles, wgpu_setup,
};
use modama_common::{
    logging::{init_logging, init_user_session_logging},
    version::modama_git_version,
};
use url::Url;

/// The Modama GUI, and the programs it runs in the user's login session
#[derive(Debug, Parser)]
#[command(version = modama_git_version())]
struct Cli {
    /// Load this configuration file instead of searching standard locations.
    #[arg(long, global = true)]
    config_path: Option<PathBuf>,

    /// Override the configured local agent URL.
    #[arg(long, global = true)]
    agent_url: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Mount the ingest shares in this session, with the mount popup and the notification-area
    /// icon. The GUI starts it when the agent reports no helper for this session.
    SessionHelper,

    /// The session helper's mount popup; talks to the helper over stdin/stdout
    #[command(hide = true)]
    MountPopup,

    /// Print the version. On Windows, redirect the output to a file to see it, or read the
    /// version in the window's footer.
    Version,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Version) => {
            print!("{}", Cli::command().render_version());
            Ok(())
        }
        Some(Command::SessionHelper) => {
            // the helper sets up its own logging (a file on Windows)
            let config = load_config(cli.config_path.as_deref(), cli.agent_url)?;
            let agent_url = Url::parse(&config.agent_url)?;
            modama_session_helper::entry::run(&agent_url)?;
            Ok(())
        }
        Some(Command::MountPopup) => {
            // needs no configuration: everything comes from the helper
            init_session_logging("mount-popup.log");
            mount_popup::run().map_err(|e| {
                log::error!("could not show the mount popup: {e}");
                e.into()
            })
        }
        None => {
            let log_path = init_session_logging("modama-agent-gui.log");
            run_gui(cli).map_err(|e| {
                log::error!("{e}");
                show_start_error(&e.to_string(), log_path.as_deref());
                e
            })
        }
    }
}

/// Log to a file on Windows (see `init_user_session_logging`), else or if that fails to stderr.
/// Returns the log file's path, if there is one.
fn init_session_logging(file_name: &str) -> Option<PathBuf> {
    match init_user_session_logging(file_name) {
        Ok(path) => path,
        Err(e) => {
            init_logging();
            log::warn!("could not log into {file_name}: {e:#}");
            None
        }
    }
}

fn run_gui(cli: Cli) -> Result<(), Box<dyn Error>> {
    let config = load_config(cli.config_path.as_deref(), cli.agent_url)?;
    let instance = match single_instance::claim() {
        Claim::Raised => return Ok(()),
        Claim::Run(instance) => instance,
    };
    let service = Arc::new(HttpAgentService::new(config.agent_url)?);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([
            styles::components::app_window::width::EGUI_POINTS,
            styles::components::app_window::height::EGUI_POINTS,
        ]),
        wgpu_options: wgpu_setup::wgpu_options(),
        ..Default::default()
    };

    eframe::run_native(
        "modama-agent",
        options,
        Box::new(move |cctx| {
            wgpu_setup::log_adapter(cctx);
            egui_extras::install_image_loaders(&cctx.egui_ctx);
            styles::theme::install(&cctx.egui_ctx);
            if let Some(instance) = instance {
                instance.listen(cctx.egui_ctx.clone());
            }
            let app = AgentGuiApp::with_service(service);
            #[cfg(feature = "autostart")]
            let app = app.with_autostart_offer();
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| match e {
        // no adapter or no surface: another backend may have one
        eframe::Error::Wgpu(_) => format!(
            "could not open the modama-agent window: {e}\n\n{}",
            wgpu_setup::BACKEND_HINT
        ),
        e => format!("could not open the modama-agent window: {e}"),
    })?;

    Ok(())
}

/// Release builds on Windows have no console, so a failed start would otherwise go unnoticed
#[cfg(windows)]
fn show_start_error(message: &str, log_path: Option<&std::path::Path>) {
    use windows::{
        Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
        core::HSTRING,
    };

    let mut text = message.to_owned();
    if let Some(path) = log_path {
        text.push_str(&format!(
            "\n\nDetails are in the log file {}",
            path.display()
        ));
    }
    // SAFETY: both strings are valid NUL-terminated UTF-16 for the duration of the call, and no
    // owner window is passed.
    // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-messageboxw
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from("modama-agent"),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_start_error(_message: &str, _log_path: Option<&std::path::Path>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn version_flag_prints_the_git_version() {
        for flag in ["--version", "-V"] {
            let error = Cli::try_parse_from(["modama-agent-gui", flag])
                .expect_err("should print the version instead of running");
            assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
            assert_eq!(
                error.to_string(),
                format!("modama-agent-gui {}\n", modama_git_version())
            );
        }
    }

    #[test]
    fn version_subcommand_parses() -> Result<(), clap::Error> {
        let cli = Cli::try_parse_from(["modama-agent-gui", "version"])?;
        assert!(matches!(cli.command, Some(Command::Version)));
        Ok(())
    }
}
