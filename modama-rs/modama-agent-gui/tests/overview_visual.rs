use eframe::egui;
use egui_kittest::{Harness, SnapshotResults, kittest::Queryable};
use modama_agent_gui::{
    app::AgentGuiApp,
    async_state::AsyncState,
    guide_screenshots::{VIEWPORT, representative_connected_agents, representative_connection},
    pages::{Page, PageConnecting, PageOverview},
    service::ConnectedAgent,
    state::{
        ActiveSession, AgentIdentity, AgentView, ForegroundSession, OperationId, PendingSession,
        SignedIn,
    },
    styles,
};
use modama_common::{
    internal_api_types::ConnectedAgent as ConnectedAgentEntry, messages::AgentMountStatus,
};
use std::sync::{Arc, Barrier};

#[test]
fn sentinel_wgpu_debug_disabled() {
    let val = std::env::var("WGPU_DEBUG").expect("WGPU_DEBUG should be set");
    assert_eq!(val, "0");
}

#[test]
fn enabled_top_bar_controls_use_pointing_hand_cursor() {
    let mut harness = overview_harness(false, false);
    harness.run_steps(3);

    harness.get_by_label("go to Ingest").hover();
    harness.run_steps(1);
    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );

    harness.remove_cursor();
    harness.run_steps(1);
    harness.get_by_label("Start Ingest").hover();
    harness.run_steps(1);
    assert_eq!(
        harness.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
}

fn overview_harness(dark: bool, logged_in: bool) -> Harness<'static, AgentGuiApp> {
    let mut harness = Harness::new_eframe(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);

        let mut app = AgentGuiApp::new();
        app.page = Page::Overview(PageOverview::new());
        app.app_state.agent.connected_agents = Some(representative_connected_agents(&[
            "detector-pc",
            "k2-gpu",
            "tem-camera",
        ]));
        if logged_in {
            app.app_state.signed_in = Some(SignedIn {
                username: "representative.user".to_string(),
            });
            app.app_state.agent.foreground = ForegroundSession::Mounted(ActiveSession {
                share_id: "representative-share".to_owned(),
                project: "representative-project".to_string(),
                session: "2026-07-29-ingest".to_string(),
                mount_path: Some(
                    "/mnt/modama/representative-project/2026-07-29-ingest".to_string(),
                ),
                created_by: Some("representative.user".to_owned()),
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

fn active_logged_out_harness() -> Harness<'static, AgentGuiApp> {
    let mut harness = overview_harness(false, true);
    harness.state_mut().app_state.signed_in = None;
    harness.run_steps(1);
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

    // long mount paths wrap, preferably after a slash
    let mut long_path = overview_harness(false, true);
    if let ForegroundSession::Mounted(session) =
        &mut long_path.state_mut().app_state.agent.foreground
    {
        session.mount_path = Some(
            "/run/user/1000/gvfs/smb-share:server=modama-storage.example.org,share=representative-project-2026-07-29-ingest/incoming/detector-frames".to_owned(),
        );
    }
    long_path.run_steps(1);
    long_path.snapshot("overview_long_mount_path");
    results.extend_harness(&mut long_path);

    // too narrow for the session and the start action side by side: one above the other
    let mut narrow = overview_harness(false, true);
    narrow.set_size(egui::vec2(520.0, VIEWPORT.y));
    narrow.run_steps(3);
    narrow.snapshot("overview_narrow");
    results.extend_harness(&mut narrow);

    for (dark, name) in [
        (false, "overview_scripts_dialog"),
        (true, "overview_scripts_dialog_dark"),
    ] {
        let mut dialog = overview_harness(dark, true);
        dialog.get_by_label("Use in scripts and notebooks").click();
        dialog.run();
        dialog.snapshot(name);
        results.extend_harness(&mut dialog);
    }

    // narrower than the dialog: it shrinks, and the code scrolls sideways
    let mut narrow_dialog = overview_harness(false, true);
    narrow_dialog.set_size(egui::vec2(520.0, VIEWPORT.y));
    narrow_dialog.run_steps(3);
    narrow_dialog
        .get_by_label("Use in scripts and notebooks")
        .click();
    narrow_dialog.run();
    narrow_dialog.snapshot("overview_scripts_dialog_narrow");
    results.extend_harness(&mut narrow_dialog);

    let mut logged_out = overview_harness(false, false);
    logged_out.snapshot("overview_logged_out");
    results.extend_harness(&mut logged_out);

    let mut mounting = overview_harness(false, false);
    mounting.state_mut().app_state.agent.foreground = ForegroundSession::Mounting(
        PendingSession {
            share_id: "mounting-share".to_owned(),
            project: "representative-project".to_owned(),
            session: "2026-07-29-ingest".to_owned(),
            created_by: Some("representative.user".to_owned()),
        },
        "Waiting for the session helper to mount the ingest share.".to_owned(),
    );
    mounting.run_steps(1);
    mounting.snapshot("overview_mounting");
    results.extend_harness(&mut mounting);

    let mut hand_off_blocked = overview_harness(false, false);
    hand_off_blocked.state_mut().app_state.agent.foreground = ForegroundSession::Mounting(
        PendingSession {
            share_id: "mounting-share".to_owned(),
            project: "representative-project".to_owned(),
            session: "2026-07-29-ingest".to_owned(),
            created_by: Some("representative.user".to_owned()),
        },
        "another user's ingest share is still connected; close their files or release it"
            .to_owned(),
    );
    hand_off_blocked
        .state_mut()
        .app_state
        .agent
        .hand_off_blocked = true;
    hand_off_blocked.run_steps(1);
    hand_off_blocked.snapshot("overview_hand_off_blocked");
    results.extend_harness(&mut hand_off_blocked);

    let mut close_confirmation = overview_harness(false, true);
    close_confirmation.get_by_label("Stop Ingest").click();
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
                Ok(())
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

    let mut error_toast = overview_harness(false, false);
    error_toast.state_mut().toast.show_error(
        "Could not load projects.".to_owned(),
        modama_agent_gui::actions::RetryAction::FetchProjects,
    );
    error_toast.run_steps(2);
    error_toast.snapshot("overview_error_toast");
    results.extend_harness(&mut error_toast);

    // only with `--features autostart`; the snapshot stays for when the feature is back
    #[cfg(feature = "autostart")]
    {
        use modama_agent_gui::components::autostart_offer::AutostartOffer;

        let mut autostart_offer = overview_harness(false, true);
        autostart_offer.state_mut().autostart_offer = Some(AutostartOffer::default());
        autostart_offer.run_steps(2);
        autostart_offer.snapshot("overview_autostart_offer");
        results.extend_harness(&mut autostart_offer);
    }

    let mut connection_dropped = overview_harness(false, true);
    connection_dropped
        .state_mut()
        .app_state
        .agent
        .connection_dropped = true;
    connection_dropped.run_steps(2);
    connection_dropped.snapshot("overview_connection_dropped");
    results.extend_harness(&mut connection_dropped);

    results.unwrap();
}

#[test]
fn dropped_connection_indicator_does_not_disable_controls() {
    let mut harness = overview_harness(false, true);
    harness.state_mut().app_state.agent.connection_dropped = true;
    harness.run_steps(2);

    assert!(
        harness
            .query_by_role_and_label(
                egui::accesskit::Role::ProgressIndicator,
                "Connection dropped; retrying"
            )
            .is_some()
    );
    for label in ["go to Ingest", "Stop Ingest", "Log out"] {
        assert!(
            harness
                .query_by(|node| node.label().as_deref() == Some(label) && !node.is_disabled())
                .is_some()
        );
    }
}

#[test]
fn overview_entry_focuses_primary_session_action() {
    let logged_out = overview_harness(false, false);
    assert!(logged_out.get_by_label("Start Ingest").is_focused());

    let logged_in = overview_harness(false, true);
    assert!(logged_in.get_by_label("Stop Ingest").is_focused());
}

#[test]
fn another_session_can_be_started_while_one_is_active() {
    let active = overview_harness(false, true);
    assert!(active.query_by_label("Stop Ingest").is_some());
    assert!(active.query_by_label("Start Ingest").is_some());

    let inactive = overview_harness(false, false);
    assert!(inactive.query_by_label("Start Ingest").is_some());
    assert!(inactive.query_by_label("Stop Ingest").is_none());
    assert!(
        inactive
            .query_by_role_and_label(egui::accesskit::Role::ProgressIndicator, "Loading")
            .is_none()
    );
}

/// The text the last frame put on the clipboard, if any
fn copied_text(harness: &Harness<'static, AgentGuiApp>) -> Option<String> {
    harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
}

#[test]
fn the_code_icon_opens_the_mount_path_and_notebook_code_to_copy() {
    let mut active = overview_harness(false, true);
    assert!(active.query_by_label("Python code").is_none());
    active.get_by_label("Use in scripts and notebooks").click();
    active.run();
    assert!(active.query_by_label("Python code").is_some());

    active
        .get_by_label("Copy mount path on this computer")
        .click();
    active.step();
    assert_eq!(
        copied_text(&active).as_deref(),
        Some("/mnt/modama/representative-project/2026-07-29-ingest")
    );

    active.get_by_label("Copy python code").click();
    active.step();
    let code = copied_text(&active).expect("the code goes to the clipboard");
    assert!(code.contains("/api/shares/foreground/"), "{code}");
    assert!(
        code.contains("session_folder = share[\"mount_path\"]"),
        "{code}"
    );

    active.get_by_label("Close").click();
    active.run();
    assert!(active.query_by_label("Python code").is_none());
}

#[test]
fn active_session_details_include_the_agent_mount_path() {
    let active = overview_harness(false, true);

    assert!(active.query_by_label("representative-project").is_some());
    assert!(active.query_by_label("2026-07-29-ingest").is_some());
    assert!(
        active
            .query_by_label("/mnt/modama/representative-project/2026-07-29-ingest")
            .is_some()
    );
}

#[test]
fn overview_logo_heading_and_card_content_share_a_left_guide() {
    let active = overview_harness(false, true);
    let logo_x = active.get_by_label("go to Ingest").rect().left();
    let heading_x = active
        .get_by_label("Ingest for this resource")
        .rect()
        .left();
    let card_content_x = active.get_by_label("Foreground ingest").rect().left();
    let tolerance = styles::components::standard_border::width::EGUI_POINTS;

    assert!((logo_x - heading_x).abs() <= tolerance);
    assert!((heading_x - card_content_x).abs() <= tolerance);
}

#[test]
fn error_toast_aligns_with_the_app_shell_geometry() {
    let mut harness = overview_harness(false, false);
    harness.state_mut().toast.show_error(
        "Could not load projects.".to_owned(),
        modama_agent_gui::actions::RetryAction::FetchProjects,
    );
    harness.run_steps(2);

    let logo = harness.get_by_label("go to Ingest").rect();
    let heading = harness.get_by_label("Ingest for this resource").rect();
    let retry = harness.get_by_label("Retry").rect();
    let dismiss = harness.get_by_label("Dismiss").rect();
    let tolerance = styles::components::standard_border::width::EGUI_POINTS * 2.0;
    let expected_right = VIEWPORT.x - styles::outer_guide_inset();
    let expected_bottom = VIEWPORT.y - styles::bottom_overlay_inset();
    let controls_right = retry.right().max(dismiss.right());

    assert!((logo.left() - heading.left()).abs() <= tolerance);
    assert!(
        (controls_right + styles::components::error_toast_padding_x::padding::EGUI_POINTS
            - expected_right)
            .abs()
            <= tolerance,
        "controls right {}, expected outer right {}, expected inner right {}",
        controls_right,
        expected_right,
        expected_right - styles::components::error_toast_padding_x::padding::EGUI_POINTS
    );
    assert!(
        (dismiss.bottom() + styles::components::error_toast_padding_y::padding::EGUI_POINTS
            - expected_bottom)
            .abs()
            <= tolerance,
        "dismiss bottom {}, expected outer bottom {}, expected inner bottom {}",
        dismiss.bottom(),
        expected_bottom,
        expected_bottom - styles::components::error_toast_padding_y::padding::EGUI_POINTS
    );
    assert!(retry.top() > logo.bottom());
    let footer_top = VIEWPORT.y - styles::components::footer::height::EGUI_POINTS;
    assert!(
        expected_bottom < footer_top,
        "the toast ends at {expected_bottom}, over the footer starting at {footer_top}"
    );
}

#[test]
fn error_toast_retry_routes_back_to_the_failed_selection_page() {
    let mut harness = overview_harness(false, false);
    harness.state_mut().app_state.signed_in = Some(SignedIn {
        username: "representative.user".to_owned(),
    });
    harness.state_mut().toast.show_error(
        "Could not load projects.".to_owned(),
        modama_agent_gui::actions::RetryAction::FetchProjects,
    );
    harness.run_steps(2);

    harness.get_by_label("Retry").click();
    harness.run_steps(1);

    assert!(matches!(harness.state().page, Page::SelectProject(_)));
}

#[test]
fn top_bar_session_action_shares_the_card_right_guide() {
    let expected_right = VIEWPORT.x - styles::outer_guide_inset();
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
    // the top bar's, not the "Started by" row's on the session card
    let username_rect = logged_in
        .get_all_by_label("representative.user")
        .map(|node| node.rect())
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the username is shown");
    assert!(
        (username_rect.center().y - logout_rect.center().y).abs() <= tolerance,
        "username and logout must remain vertically centered on the same text line"
    );
}

/// DESIGN.md › Layout › Guides and › Footer: the version ends on the inner right guide, the
/// footer has its fixed height, and bottom-anchored overlays stay above it.
#[test]
fn footer_version_sits_on_the_inner_right_guide() {
    let harness = overview_harness(false, false);
    let tolerance = styles::components::standard_border::width::EGUI_POINTS;
    let version = harness
        .get_by_label(modama_agent_gui::app::STUB_VERSION)
        .rect();
    let expected_right = VIEWPORT.x - styles::inner_guide_inset();
    assert!(
        (version.right() - expected_right).abs() <= tolerance,
        "version right edge {} does not match the inner guide {expected_right}",
        version.right()
    );
    let footer_top = VIEWPORT.y - styles::components::footer::height::EGUI_POINTS;
    assert!(
        version.top() > footer_top && version.bottom() < VIEWPORT.y,
        "version {version:?} is not inside the footer starting at {footer_top}"
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

        if logged_in {
            // focus starts on "Stop Ingest"; "Start Ingest" below it comes next
            press_tab(&mut harness, false);
            assert!(harness.get_by_label("Start Ingest").is_focused());
        }
        press_tab(&mut harness, false);
        assert!(harness.get_by_label("go to Ingest").is_focused());

        press_tab(&mut harness, false);
        assert!(harness.get_by_label("Switch to dark theme").is_focused());

        press_tab(&mut harness, false);
        assert!(harness.get_by_label(account_action).is_focused());

        press_tab(&mut harness, true);
        assert!(harness.get_by_label("Switch to dark theme").is_focused());

        press_tab(&mut harness, true);
        assert!(harness.get_by_label("go to Ingest").is_focused());
    }
}

#[test]
fn missing_agent_mount_path_is_explained() {
    let mut active = overview_harness(false, true);
    let ForegroundSession::Mounted(session) = &mut active.state_mut().app_state.agent.foreground
    else {
        panic!("the harness should have an active session");
    };
    session.mount_path = None;
    active.run_steps(1);

    assert!(
        active
            .query_by_label("Not available on this computer")
            .is_some()
    );
}

#[test]
fn closing_requires_confirmation_and_can_be_cancelled() {
    let mut harness = overview_harness(false, true);
    harness.get_by_label("Stop Ingest").click();
    harness.run();

    assert!(matches!(
        harness.state().app_state.agent.foreground,
        ForegroundSession::Mounted(_)
    ));
    assert!(harness.query_by_label("Stop ingest?").is_some());
    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(matches!(
        harness.state().app_state.agent.foreground,
        ForegroundSession::Mounted(_)
    ));
    assert!(harness.query_by_label("Stop ingest?").is_none());
}

#[test]
fn logged_out_close_authenticates_before_showing_confirmation() {
    let mut harness = active_logged_out_harness();

    harness.get_by_label("Stop Ingest").click();
    harness.run_steps(2);

    assert!(matches!(harness.state().page, Page::Login(_)));
    assert!(harness.query_by_label("Stop ingest?").is_none());

    if let Page::Login(login) = &mut harness.state_mut().page {
        login.login_task = AsyncState::Ready {
            payload: Ok(SignedIn {
                username: "representative.user".to_owned(),
            }),
        };
    }
    harness.run_steps(2);

    assert!(matches!(harness.state().page, Page::Overview(_)));
    assert!(harness.query_by_label("Stop ingest?").is_some());
    assert!(harness.get_by_label("Cancel").is_focused());
}

#[test]
fn backing_out_of_close_login_discards_the_confirmation_destination() {
    let mut harness = active_logged_out_harness();
    harness.get_by_label("Stop Ingest").click();
    harness.run_steps(2);

    harness.get_by_label("← Back to overview").click();
    harness.run_steps(2);

    assert!(matches!(harness.state().page, Page::Overview(_)));
    assert!(harness.query_by_label("Stop ingest?").is_none());
    assert!(harness.query_by_label("Stop Ingest").is_some());
}

#[test]
fn escape_dismisses_close_confirmation() {
    let mut harness = overview_harness(false, true);
    harness.get_by_label("Stop Ingest").click();
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();

    assert!(matches!(
        harness.state().app_state.agent.foreground,
        ForegroundSession::Mounted(_)
    ));
    assert!(harness.query_by_label("Stop ingest?").is_none());
}

#[test]
fn confirming_close_waits_for_the_authoritative_update() {
    let mut harness = overview_harness(false, true);
    harness.get_by_label("Stop Ingest").click();
    harness.run();
    harness
        .get_all_by_label("Stop Ingest")
        .last()
        .expect("confirmation button was not found")
        .click();
    harness.run_steps(3);

    assert_eq!(
        harness.state().app_state.pending_close(),
        Some("representative-share")
    );
    assert!(harness.query_by_label("Stopping…").is_some());

    let app_state = &mut harness.state_mut().app_state;
    app_state.apply_agent_view(AgentView {
        foreground: ForegroundSession::None,
        ..app_state.agent.clone()
    });
    harness.run_steps(1);

    assert!(harness.query_by_label("Start Ingest").is_some());
    assert!(harness.query_by_label("Stop Ingest").is_none());
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
                    ConnectedAgent {
                        identity: modama_agent_gui::state::AgentIdentity::Anonymous {
                            hostname: "snapshot-host".to_owned(),
                        },
                        foreground: ForegroundSession::None,
                    }
                },
                cc.egui_ctx.clone(),
            );
        }
        app
    });
    harness.set_size(VIEWPORT).set_pixels_per_point(1.0);
    harness.ctx.set_theme(egui::Theme::Light);

    harness.run_steps(3);
    assert!(
        harness
            .query_by(|node| node.label().as_deref() == Some("Log in") && node.is_disabled())
            .is_some()
    );
    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("go to Ingest") && node.is_disabled()
            })
            .is_some()
    );
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::ProgressIndicator, "Loading")
            .is_some()
    );
    harness.snapshot("connecting");
    release.wait();

    let mut results = SnapshotResults::new();
    results.extend_harness(&mut harness);
    results.unwrap();
}

#[test]
fn started_by_is_shown_only_when_known() {
    let mut known = overview_harness(false, true);
    known.run_steps(1);
    assert!(known.query_by_label("Started by").is_some());

    let mut unknown = overview_harness(false, true);
    if let ForegroundSession::Mounted(session) = &mut unknown.state_mut().app_state.agent.foreground
    {
        session.created_by = None;
    }
    unknown.run_steps(1);
    assert!(unknown.query_by_label("Started by").is_none());
}

#[test]
fn start_while_open_snapshots() {
    let mut results = SnapshotResults::new();

    // the user's own share is open: keep it, or close it once the new one has started
    let mut own = overview_harness(false, true);
    own.get_by_label("Start Ingest").click();
    own.run();
    own.snapshot("overview_start_while_own_share_open");
    results.extend_harness(&mut own);

    // someone else's share: starting is a hand-off
    let mut others = overview_harness(false, true);
    others.state_mut().app_state.identity = Some(AgentIdentity::AgentId {
        agent_name: "k2".to_owned(),
        resource_name: "Holo".to_owned(),
    });
    if let ForegroundSession::Mounted(session) = &mut others.state_mut().app_state.agent.foreground
    {
        session.created_by = Some("other.user".to_owned());
    }
    others.run_steps(1);
    others.get_by_label("Start Ingest").click();
    others.run();
    others.snapshot("overview_start_while_others_share_open");
    results.extend_harness(&mut others);

    // the new session is being prepared while the previous share is still open
    let mut pending = overview_harness(false, true);
    let operation_id = OperationId::next();
    let app_state = &mut pending.state_mut().app_state;
    app_state.begin_start(
        operation_id,
        "representative-project".to_owned(),
        "2026-07-30-ingest".to_owned(),
    );
    app_state.finish_start(
        operation_id,
        PendingSession {
            share_id: "new-share".to_owned(),
            project: "representative-project".to_owned(),
            session: "2026-07-30-ingest".to_owned(),
            created_by: None,
        },
    );
    pending.run_steps(1);
    pending.snapshot("overview_start_pending_while_share_open");
    results.extend_harness(&mut pending);
}

fn other_mount(share_id: &str, session: &str) -> ActiveSession {
    ActiveSession {
        share_id: share_id.to_owned(),
        project: "representative-project".to_owned(),
        session: session.to_owned(),
        mount_path: Some(format!("/mnt/modama/representative-project/{session}")),
        created_by: Some("representative.user".to_owned()),
    }
}

/// The agent reports `other_mounts` next to the shown foreground share
fn show_other_mounts(
    harness: &mut Harness<'static, AgentGuiApp>,
    other_mounts: Vec<ActiveSession>,
) {
    let app_state = &mut harness.state_mut().app_state;
    app_state.apply_agent_view(AgentView {
        other_mounts,
        ..app_state.agent.clone()
    });
}

fn with_other_mounts(harness: &mut Harness<'static, AgentGuiApp>) {
    show_other_mounts(
        harness,
        vec![
            other_mount("older-share", "2026-07-27-ingest"),
            other_mount("old-share", "2026-07-28-ingest"),
        ],
    );
    harness.run_steps(1);
}

#[test]
fn other_mounts_snapshots() {
    let mut results = SnapshotResults::new();

    let mut listed = overview_harness(false, true);
    with_other_mounts(&mut listed);
    listed.snapshot("overview_other_mounts");
    results.extend_harness(&mut listed);

    let mut confirm = overview_harness(false, true);
    with_other_mounts(&mut confirm);
    // labels match exactly: these are the list's buttons, not "Stop Ingest"
    assert_eq!(confirm.get_all_by_label("Stop").count(), 2);
    // the list may start below the window's edge
    confirm
        .get_all_by_label("Stop")
        .next()
        .expect("the list has a stop button")
        .scroll_to_me();
    confirm.run();
    confirm
        .get_all_by_label("Stop")
        .next()
        .expect("the list has a stop button")
        .click();
    confirm.run();
    assert!(
        confirm.query_by_label("Stop ingest?").is_some(),
        "the click should open the confirmation"
    );
    confirm.snapshot("overview_other_mount_close_confirmation");
    results.extend_harness(&mut confirm);

    // the cards are taller than the window: they scroll below the heading
    let mut scrolled = overview_harness(false, true);
    show_other_mounts(
        &mut scrolled,
        (21..=28)
            .map(|day| other_mount(&format!("share-{day}"), &format!("2026-07-{day}-ingest")))
            .collect(),
    );
    scrolled.run_steps(1);
    scrolled.event(egui::Event::PointerMoved(egui::pos2(500.0, 400.0)));
    scrolled.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -2000.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    // scrolling is animated
    scrolled.run_steps(30);
    assert!(
        scrolled
            .query_by_label("representative-project / 2026-07-28-ingest · representative.user")
            .is_some()
    );
    scrolled.snapshot("overview_other_mounts_scrolled");
    results.extend_harness(&mut scrolled);

    let mut closing = overview_harness(false, true);
    with_other_mounts(&mut closing);
    closing
        .state_mut()
        .app_state
        .begin_close(OperationId::next(), "old-share".to_owned());
    closing.run_steps(1);
    closing.snapshot("overview_other_mount_closing");
    results.extend_harness(&mut closing);
}

#[test]
fn connected_agents_snapshots() {
    let mut results = SnapshotResults::new();

    // more agents than the list shows: it scrolls on its own
    let mut scrolled = overview_harness(false, true);
    scrolled.state_mut().app_state.agent.connected_agents =
        Some(representative_connected_agents(&[
            "detector-pc",
            "eels-spectrometer",
            "k2-gpu",
            "merlin-detector",
            "plasma-cleaner",
            "stage-controller",
            "tem-camera",
            "tomography-pc",
        ]));
    scrolled.run_steps(1);
    let list = scrolled.get_by_label("tomography-pc").rect();
    scrolled.event(egui::Event::PointerMoved(egui::pos2(
        list.center().x,
        scrolled.get_by_label("detector-pc").rect().center().y,
    )));
    scrolled.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -2000.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    // scrolling is animated
    scrolled.run_steps(30);
    // the page itself didn't scroll
    assert!(scrolled.query_by_label("Start Ingest").is_some());
    scrolled.snapshot("overview_connected_agents_scrolled");
    results.extend_harness(&mut scrolled);

    // one agent per mount status, scrolled to the last one, the pointer on its (unknown) ring
    let mut statuses = overview_harness(false, true);
    statuses.state_mut().app_state.agent.connected_agents =
        Some(representative_connected_agents(&[
            "detector-pc",
            "k2-gpu",
            "merlin-detector",
            "tem-camera",
        ]));
    statuses.run_steps(1);
    let list = statuses.get_by_label("detector-pc").rect();
    statuses.event(egui::Event::PointerMoved(list.center()));
    statuses.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -2000.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    // scrolling is animated
    statuses.run_steps(30);
    let name = statuses.get_by_label("tem-camera").rect();
    let spacing = statuses.ctx.global_style().spacing.item_spacing.x;
    let dot_radius = styles::components::connected_agent_dot::size::EGUI_POINTS / 2.0;
    statuses.event(egui::Event::PointerMoved(egui::pos2(
        name.left() - spacing - dot_radius,
        name.center().y,
    )));
    // tooltips show after a delay
    statuses.run_steps(60);
    assert!(statuses.query_by_label("status unknown").is_some());
    statuses.snapshot("overview_connected_agents_statuses");
    results.extend_harness(&mut statuses);

    let mut alone = overview_harness(false, true);
    alone.state_mut().app_state.agent.connected_agents = Some(representative_connected_agents(&[]));
    alone.run_steps(1);
    assert!(
        alone
            .query_by_label("No other agents are connected.")
            .is_some()
    );
    alone.snapshot("overview_connected_agents_alone");
    results.extend_harness(&mut alone);

    // an anonymous agent: its own row has an (i) instead of the lock; it lists the agents of
    // the resource it inherited from its token. The pointer is on its (i).
    let mut anonymous = overview_harness(false, true);
    let mut agents = representative_connected_agents(&["detector-pc", "k2"]);
    agents.own = ConnectedAgentEntry {
        name: "workstation".to_owned(),
        verified: false,
        status: Some(AgentMountStatus::Idle),
        connection: representative_connection(9, Some("k2")),
    };
    anonymous.state_mut().app_state.agent.connected_agents = Some(agents);
    // two frames: in the first, egui's debug build flags the own row's tag, whose ID changed
    // with the row's contents
    anonymous.run_steps(2);
    let info = anonymous
        .get_by_label(egui_phosphor::regular::INFO)
        .rect()
        .center();
    anonymous.event(egui::Event::PointerMoved(info));
    // tooltips show after a delay
    anonymous.run_steps(60);
    assert!(anonymous.query_by_label_contains("Token from k2").is_some());
    anonymous.snapshot("overview_connected_agents_anonymous");
    results.extend_harness(&mut anonymous);
}

#[test]
fn connected_agents_are_shown_even_before_they_are_known() {
    let mut harness = overview_harness(false, true);
    harness.state_mut().app_state.agent.connected_agents = None;
    harness.run_steps(1);
    assert!(harness.query_by_label("Connected agents").is_some());
    assert!(
        harness
            .query_by_label("Not connected to the server yet.")
            .is_some()
    );
}
