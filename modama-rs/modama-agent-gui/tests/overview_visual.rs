use eframe::egui;
use egui_kittest::{Harness, SnapshotResults};
use modama_agent_gui::{
    app::AgentGuiApp,
    pages::{Page, PageOverview},
    state::{ActiveSession, Credentials},
    styles,
};
use std::sync::{Arc, Barrier};

const VIEWPORT: egui::Vec2 = egui::vec2(1024.0, 768.0);

fn overview_harness(dark: bool, logged_in: bool) -> Harness<'static, AgentGuiApp> {
    let mut harness = Harness::new_eframe(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);

        let mut app = AgentGuiApp::new();
        app.page = Page::Overview(PageOverview::new());
        if logged_in {
            app.app_state.creds = Some(Credentials {
                username: "representative.user".to_string(),
                password: "representative-password".to_string(),
            });
            app.app_state.session = Some(ActiveSession {
                project: "representative-project".to_string(),
                session: "2026-07-29-ingest".to_string(),
            });
        }
        app
    });
    harness.set_size(VIEWPORT).set_pixels_per_point(1.0);
    harness.ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    harness.run_steps(3);
    harness
}

#[test]
fn overview_snapshots() {
    let mut results = SnapshotResults::new();

    let mut logged_in_light = overview_harness(false, true);
    logged_in_light.snapshot("overview_logged_in_light");
    results.extend_harness(&mut logged_in_light);

    let mut logged_in_dark = overview_harness(true, true);
    logged_in_dark.snapshot("overview_logged_in_dark");
    results.extend_harness(&mut logged_in_dark);

    let mut logged_out = overview_harness(false, false);
    logged_out.snapshot("overview_logged_out");
    results.extend_harness(&mut logged_out);

    results.unwrap();
}

#[test]
fn connecting_snapshot() {
    let mut harness = Harness::new_eframe(|cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);
        AgentGuiApp::new()
    });
    harness.set_size(VIEWPORT).set_pixels_per_point(1.0);
    harness.ctx.set_theme(egui::Theme::Light);

    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let ctx = harness.ctx.clone();
    if let Page::Connecting(connecting) = &mut harness.state_mut().page {
        connecting.connect_task.spawn(
            move || {
                task_release.wait();
                Ok(())
            },
            ctx,
        );
    }

    harness.run_steps(3);
    harness.snapshot("connecting");
    release.wait();

    let mut results = SnapshotResults::new();
    results.extend_harness(&mut harness);
    results.unwrap();
}
