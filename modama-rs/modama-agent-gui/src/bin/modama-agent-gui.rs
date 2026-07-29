use std::error::Error;

use eframe::egui;
use modama_agent_gui::{app::AgentGuiApp, styles};

fn main() -> Result<(), Box<dyn Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 768.0]),
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
