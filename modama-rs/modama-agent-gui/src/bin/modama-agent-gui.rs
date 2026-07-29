use std::error::Error;

use eframe::egui;
use modama_agent_gui::{app::AgentGuiApp, styles};

fn main() -> Result<(), Box<dyn Error>> {
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
        Box::new(|cctx| {
            egui_extras::install_image_loaders(&cctx.egui_ctx);
            styles::theme::install(&cctx.egui_ctx);
            let app = AgentGuiApp::new();
            Ok(Box::new(app))
        }),
    )?;

    Ok(())
}
