use std::{error::Error, path::PathBuf, sync::Arc};

use clap::Parser;
use eframe::egui;
use modama_agent_gui::{
    app::AgentGuiApp, config::load_config, helper_launch::launch_session_helper, mount_popup,
    service::HttpAgentService, styles,
};
use modama_common::logging::init_logging;

#[derive(Debug, Parser)]
struct Cli {
    /// Load this configuration file instead of searching standard locations.
    #[arg(long)]
    config_path: Option<PathBuf>,

    /// Override the configured local agent URL.
    #[arg(long)]
    agent_url: Option<String>,

    /// Start the session helper without a window and exit (used by the autostart entry).
    #[arg(long)]
    start_session_helper: bool,

    /// Show the mount popup for the session helper, which talks to it over stdin/stdout.
    #[arg(long)]
    mount_popup: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    init_logging();

    let cli = Cli::parse();
    if cli.mount_popup {
        // needs no configuration: everything comes from the helper
        mount_popup::run()?;
        return Ok(());
    }
    let config = load_config(cli.config_path.as_deref(), cli.agent_url)?;
    if cli.start_session_helper {
        // nobody sees this process, so only log; the helper keeps running after we exit
        if let Err(e) = launch_session_helper(&config.agent_url) {
            log::error!("could not launch the session helper: {e}");
        }
        return Ok(());
    }
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
            let app = AgentGuiApp::with_service(service).with_autostart_offer();
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
