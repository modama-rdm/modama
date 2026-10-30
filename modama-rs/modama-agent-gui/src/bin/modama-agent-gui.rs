// No console window on Windows: the GUI and the always-running session helper are both started
// from this executable. Debug builds keep the console for their log output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{error::Error, path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use eframe::egui;
use modama_agent_gui::{
    app::AgentGuiApp, config::load_config, mount_popup, service::HttpAgentService, styles,
};
use modama_common::logging::init_logging;
use url::Url;

/// The Modama GUI, and the programs it runs in the user's session
#[derive(Debug, Parser)]
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
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::SessionHelper) => {
            // the helper sets up its own logging (a file on Windows)
            let config = load_config(cli.config_path.as_deref(), cli.agent_url)?;
            let agent_url = Url::parse(&config.agent_url)?;
            modama_session_helper::entry::run(&agent_url)?;
            return Ok(());
        }
        Some(Command::MountPopup) => {
            // needs no configuration: everything comes from the helper
            init_logging();
            mount_popup::run()?;
            return Ok(());
        }
        None => init_logging(),
    }

    let config = load_config(cli.config_path.as_deref(), cli.agent_url)?;
    let service = Arc::new(HttpAgentService::new(config.agent_url)?);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([
            styles::components::app_window::width::EGUI_POINTS,
            styles::components::app_window::height::EGUI_POINTS,
        ]),
        #[cfg(target_os = "linux")]
        wgpu_options: modama_agent_gui::wgpu_setup::wgpu_options(),
        ..Default::default()
    };

    eframe::run_native(
        "modama-agent",
        options,
        Box::new(move |cctx| {
            egui_extras::install_image_loaders(&cctx.egui_ctx);
            styles::theme::install(&cctx.egui_ctx);
            let app = AgentGuiApp::with_service(service);
            #[cfg(feature = "autostart")]
            let app = app.with_autostart_offer();
            Ok(Box::new(app))
        }),
    )?;

    Ok(())
}
