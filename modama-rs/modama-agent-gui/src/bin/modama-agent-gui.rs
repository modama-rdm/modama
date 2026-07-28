use std::error::Error;

use eframe::egui;

use crate::app::AgentGuiApp;

fn main() -> Result<(), Box<dyn Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024., 1024.0]),
        ..Default::default()
    };

    eframe::run_native("modama-agent", options, Box::new(|cctx| {
        egui_extras::install_image_loaders(&cctx.egui_ctx);
        let app = AgentGuiApp::new();
        Ok(Box::new(app))
    }))?;

    Ok(())
}
