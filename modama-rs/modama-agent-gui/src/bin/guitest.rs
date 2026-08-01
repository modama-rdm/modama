use std::error::Error;

use eframe::egui;
use modama_agent_gui::{
    app::AgentGuiApp,
    pages::{Page, PageSelectCreateSession},
    styles,
};

fn main() -> Result<(), Box<dyn Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([
            styles::components::app_window_width::width::EGUI_POINTS,
            styles::components::app_window_height::height::EGUI_POINTS,
        ]),
        ..Default::default()
    };

    eframe::run_native(
        "modama-agent-test-gui",
        options,
        Box::new(|cctx| {
            egui_extras::install_image_loaders(&cctx.egui_ctx);
            styles::theme::install(&cctx.egui_ctx);
            let mut app = AgentGuiApp::new();
            app.page = Page::SelectCreateSession(PageSelectCreateSession::new("what".to_owned()));

            let dark = true;
            cctx.egui_ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });

            Ok(Box::new(app))
        }),
    )?;

    Ok(())
}
