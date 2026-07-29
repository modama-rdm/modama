use eframe::egui;
use egui_kittest::{kittest::Queryable, Harness, SnapshotResults};
use modama_agent_gui::{
    app::AgentGuiApp,
    pages::{Page, PageLogin},
    styles,
};
use std::sync::{Arc, Barrier};

const VIEWPORT: egui::Vec2 = egui::vec2(1024.0, 1024.0);

fn login_harness(dark: bool) -> Harness<'static, AgentGuiApp> {
    let mut harness = Harness::new_eframe(|cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);

        let mut app = AgentGuiApp::new();
        app.page = Page::Login(PageLogin::new());
        app
    });
    harness.set_size(VIEWPORT).set_pixels_per_point(1.0);
    harness.ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    assert_eq!(harness.ctx.global_style().visuals.dark_mode, dark);
    // Let the SVG loader and the renderer settle before capturing the frame.
    harness.run_steps(3);
    harness
}

#[test]
fn login_snapshots() {
    let mut results = SnapshotResults::new();

    let mut light = login_harness(false);
    light.snapshot("login_default_light");
    results.extend_harness(&mut light);

    let mut dark = login_harness(true);
    dark.snapshot("login_default_dark");
    results.extend_harness(&mut dark);

    let mut focused = login_harness(false);
    let username = focused
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .next()
        .unwrap_or_else(|| panic!("username text input was not found"));
    username.focus();
    username.type_text("representative.user");
    focused.run();
    focused.snapshot("login_focused_username");
    results.extend_harness(&mut focused);

    let mut error = login_harness(false);
    if let Page::Login(login) = &mut error.state_mut().page {
        login.username = "representative.user".to_string();
        login.password = "representative-password".to_string();
        login.login_error =
            Some("Unable to authenticate with the supplied credentials.".to_string());
    }
    error.run();
    error.snapshot("login_full_width_error");
    results.extend_harness(&mut error);

    let mut pending = login_harness(false);
    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let ctx = pending.ctx.clone();
    if let Page::Login(login) = &mut pending.state_mut().page {
        login.username = "representative.user".to_string();
        login.password = "representative-password".to_string();
        login.login_task.spawn(
            move || {
                task_release.wait();
                Err("pending login snapshot complete".to_string())
            },
            ctx,
        );
    }
    pending.run_steps(1);
    pending.snapshot("login_pending");
    results.extend_harness(&mut pending);
    release.wait();

    results.unwrap();
}
