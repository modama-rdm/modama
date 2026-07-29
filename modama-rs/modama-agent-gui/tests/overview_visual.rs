use eframe::egui;
use egui_kittest::{kittest::Queryable, Harness, SnapshotResults};
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
                mount_path: None,
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

    let mut close_confirmation = overview_harness(false, true);
    close_confirmation.get_by_label("Close Session").click();
    close_confirmation.run();
    close_confirmation.snapshot("overview_close_confirmation");
    results.extend_harness(&mut close_confirmation);

    let close_release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&close_release);
    let mut close_pending = overview_harness(false, true);
    let ctx = close_pending.ctx.clone();
    if let Page::Overview(page) = &mut close_pending.state_mut().page {
        page.close_task.spawn(
            move || {
                task_release.wait();
            },
            ctx,
        );
    }
    close_pending.run_steps(1);
    close_pending.snapshot("overview_close_pending");
    close_release.wait();
    results.extend_harness(&mut close_pending);

    let mut logout_pending = overview_harness(false, true);
    logout_pending.get_by_label("Log out").click();
    logout_pending.run_steps(3);
    assert!(logout_pending.query_by_label("Logging out…").is_some());
    logout_pending.snapshot("overview_logout_pending");
    results.extend_harness(&mut logout_pending);

    results.unwrap();
}

#[test]
fn overview_actions_are_mutually_exclusive() {
    let active = overview_harness(false, true);
    assert!(active.query_by_label("Close Session").is_some());
    assert!(active.query_by_label("Create Session").is_none());

    let inactive = overview_harness(false, false);
    assert!(inactive.query_by_label("Create Session").is_some());
    assert!(inactive.query_by_label("Close Session").is_none());
}

#[test]
fn closing_requires_confirmation_and_can_be_cancelled() {
    let mut harness = overview_harness(false, true);
    harness.get_by_label("Close Session").click();
    harness.run();

    assert!(harness.state().app_state.session.is_some());
    assert!(harness.query_by_label("Close ingest session?").is_some());
    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().app_state.session.is_some());
    assert!(harness.query_by_label("Close ingest session?").is_none());
}

#[test]
fn escape_dismisses_close_confirmation() {
    let mut harness = overview_harness(false, true);
    harness.get_by_label("Close Session").click();
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();

    assert!(harness.state().app_state.session.is_some());
    assert!(harness.query_by_label("Close ingest session?").is_none());
}

#[test]
fn confirming_close_clears_the_active_session() {
    let mut harness = overview_harness(false, true);
    harness.get_by_label("Close Session").click();
    harness.run();
    harness
        .get_all_by_label("Close Session")
        .last()
        .expect("confirmation button was not found")
        .click();
    harness.run_steps(3);

    assert!(harness.state().page.is_busy());
    assert!(harness.query_by_label("Closing…").is_some());

    std::thread::sleep(std::time::Duration::from_millis(550));
    harness.run_steps(2);

    assert!(harness.state().app_state.session.is_none());
    assert!(harness.query_by_label("Create Session").is_some());
    assert!(harness.query_by_label("Close Session").is_none());
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
