use eframe::egui;
use egui_kittest::{kittest::Queryable, Harness, SnapshotResults};
use modama_agent_gui::{
    app::AgentGuiApp,
    pages::{Page, PageConnecting, PageOverview},
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
                mount_path: Some(
                    "/mnt/modama/representative-project/2026-07-29-ingest".to_string(),
                ),
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

    let mut logout_completed = overview_harness(false, true);
    logout_completed.get_by_label("Log out").click();
    logout_completed.run_steps(3);
    assert!(logout_completed.query_by_label("Log out").is_none());
    logout_completed.snapshot("overview_logout_completed");
    results.extend_harness(&mut logout_completed);

    results.unwrap();
}

#[test]
fn overview_entry_focuses_primary_session_action() {
    let logged_out = overview_harness(false, false);
    assert!(logged_out.get_by_label("Create Session").is_focused());

    let logged_in = overview_harness(false, true);
    assert!(logged_in.get_by_label("Close Session").is_focused());
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
fn active_session_details_include_the_agent_mount_path() {
    let active = overview_harness(false, true);

    assert!(active.query_by_label("representative-project").is_some());
    assert!(active.query_by_label("2026-07-29-ingest").is_some());
    assert!(active
        .query_by_label("/mnt/modama/representative-project/2026-07-29-ingest")
        .is_some());
}

#[test]
fn overview_logo_heading_and_card_content_share_a_left_guide() {
    let active = overview_harness(false, true);
    let logo_x = active.get_by_label("go to Overview").rect().left();
    let heading_x = active.get_by_label("Overview").rect().left();
    let card_content_x = active.get_by_label("Active ingest session").rect().left();
    let tolerance = styles::components::standard_border::width::EGUI_POINTS;

    assert!((logo_x - heading_x).abs() <= tolerance);
    assert!((heading_x - card_content_x).abs() <= tolerance);
}

#[test]
fn top_bar_session_action_shares_the_card_right_guide() {
    let expected_right = VIEWPORT.x - styles::content_card_outer_inset_from_window();
    let tolerance = styles::components::standard_border::width::EGUI_POINTS;

    let logged_out = overview_harness(false, false);
    let login_right = logged_out.get_by_label("Log in").rect().right();
    assert!(
        (login_right - expected_right).abs() <= tolerance,
        "login right edge {login_right} does not match guide {expected_right}"
    );

    let logged_in = overview_harness(false, true);
    let logout_rect = logged_in.get_by_label("Log out").rect();
    let logout_right = logout_rect.right();
    assert!(
        (logout_right - expected_right).abs() <= tolerance,
        "logout right edge {logout_right} does not match guide {expected_right}"
    );
    let username_rect = logged_in.get_by_label("representative.user").rect();
    assert!(
        (username_rect.center().y - logout_rect.center().y).abs() <= tolerance,
        "username and logout must remain vertically centered on the same text line"
    );
}

fn press_tab(harness: &mut Harness<'static, AgentGuiApp>, reverse: bool) {
    if reverse {
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    } else {
        harness.key_press(egui::Key::Tab);
    }
    harness.run_steps(1);
}

#[test]
fn top_bar_tab_order_follows_its_visual_order() {
    for (logged_in, account_action) in [(false, "Log in"), (true, "Log out")] {
        let mut harness = overview_harness(false, logged_in);

        press_tab(&mut harness, false);
        assert!(harness.get_by_label("go to Overview").is_focused());

        press_tab(&mut harness, false);
        assert!(harness.get_by_label("Switch to dark theme").is_focused());

        press_tab(&mut harness, false);
        assert!(harness.get_by_label(account_action).is_focused());

        press_tab(&mut harness, true);
        assert!(harness.get_by_label("Switch to dark theme").is_focused());

        press_tab(&mut harness, true);
        assert!(harness.get_by_label("go to Overview").is_focused());
    }
}

#[test]
fn missing_agent_mount_path_is_explained() {
    let mut active = overview_harness(false, true);
    active
        .state_mut()
        .app_state
        .session
        .as_mut()
        .expect("the harness should have an active session")
        .mount_path = None;
    active.run_steps(1);

    assert!(active
        .query_by_label("Not available on this agent")
        .is_some());
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

    assert!(harness.state().app_state.session.is_none());
    assert!(harness.query_by_label("Create Session").is_some());
    assert!(harness.query_by_label("Close Session").is_none());
}

#[test]
fn connecting_snapshot() {
    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let mut harness = Harness::new_eframe(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);
        let mut app = AgentGuiApp::new();
        app.page = Page::Connecting(PageConnecting::new());
        if let Page::Connecting(connecting) = &mut app.page {
            connecting.connect_task.spawn(
                move || {
                    task_release.wait();
                    Ok(())
                },
                cc.egui_ctx.clone(),
            );
        }
        app
    });
    harness.set_size(VIEWPORT).set_pixels_per_point(1.0);
    harness.ctx.set_theme(egui::Theme::Light);

    harness.run_steps(3);
    harness.snapshot("connecting");
    release.wait();

    let mut results = SnapshotResults::new();
    results.extend_harness(&mut harness);
    results.unwrap();
}
