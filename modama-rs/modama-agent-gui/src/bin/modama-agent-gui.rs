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
        wgpu_options: wgpu_options(),
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

/// Leaves out wgpu's GL backend when a primary backend (Vulkan) has an adapter.
///
/// The default setup initialises GL while enumerating adapters. Over a forwarded X11
/// connection, Mesa's EGL then sends a `DRI2Connect` that the X server rejects, and winit
/// aborts on the resulting X error. GL stays enabled when Vulkan has no adapter, and
/// `WGPU_BACKEND` still overrides the choice.
#[cfg(target_os = "linux")]
fn wgpu_options() -> eframe::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuSetup, wgpu};

    let mut config = eframe::WgpuConfiguration::default();
    if wgpu::Backends::from_env().is_some() {
        return config;
    }
    let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup else {
        return config;
    };

    // Without a display handle, enumerating Vulkan adapters does not talk to the X server.
    let probe = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
    });
    let adapters = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(runtime) => runtime.block_on(probe.enumerate_adapters(wgpu::Backends::PRIMARY)),
        Err(err) => {
            log::warn!("Could not probe for wgpu adapters, keeping default backends: {err}");
            return config;
        }
    };
    if adapters.is_empty() {
        log::info!("No Vulkan adapter found, keeping the GL backend enabled");
    } else {
        setup.instance_descriptor.backends = wgpu::Backends::PRIMARY;
    }
    config
}
