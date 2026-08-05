use std::{error::Error, path::PathBuf, sync::Arc};

use clap::Parser;
use eframe::egui;
use modama_agent_gui::{app::AgentGuiApp, config::load_config, service::HttpAgentService, styles};
use modama_common::logging::init_logging;

#[derive(Debug, Parser)]
struct Cli {
    /// Load this configuration file instead of searching standard locations.
    #[arg(long)]
    config_path: Option<PathBuf>,

    /// Override the configured local agent URL.
    #[arg(long)]
    agent_url: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    init_logging();

    let cli = Cli::parse();
    let config = load_config(cli.config_path.as_deref(), cli.agent_url)?;
    let service = Arc::new(HttpAgentService::new(config.agent_url)?);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([
            styles::components::app_window_width::width::EGUI_POINTS,
            styles::components::app_window_height::height::EGUI_POINTS,
        ]),
        ..Default::default()
    };

    eframe::run_native(
        "modama-agent",
        options,
        Box::new(move |cctx| {
            egui_extras::install_image_loaders(&cctx.egui_ctx);
            styles::theme::install(&cctx.egui_ctx);
            let app = AgentGuiApp::with_service(service);
            Ok(Box::new(app))
        }),
    )?;

    Ok(())
}
