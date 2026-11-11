use eframe::egui;
use egui_kittest::{Harness, SnapshotResults, kittest::Queryable};
use modama_agent_gui::{
    app::AgentGuiApp,
    async_state::AsyncState,
    guide_screenshots::freeze_time,
    pages::{Page, PageLogin},
    service::ServiceError,
    styles,
};
use std::sync::{Arc, Barrier};

const VIEWPORT: egui::Vec2 = egui::vec2(1024.0, 1024.0);

#[test]
fn sentinel_wgpu_debug_disabled() {
    let val = std::env::var("WGPU_DEBUG").expect("WGPU_DEBUG should be set");
    assert_eq!(val, "0");
}

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
fn enter_submits_from_each_login_input() {
    for input_role in [
        egui::accesskit::Role::TextInput,
        egui::accesskit::Role::PasswordInput,
    ] {
        let mut harness = login_harness(false);
        if let Page::Login(login) = &mut harness.state_mut().page {
            login.username = "representative.user".to_string();
            login.password = "representative-password".to_string();
        }
        harness.run();

        let input = harness
            .get_all_by_role(input_role)
            .next()
            .unwrap_or_else(|| panic!("login input with role {input_role:?} was not found"));
        input.focus();
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run_steps(1);

        match &harness.state().page {
            Page::Login(login) => assert!(
                login.login_task.is_pending(),
                "Enter did not submit login input with role {input_role:?}"
            ),
            Page::SelectProject(_) => {}
            _ => panic!("login submission changed to an unexpected page"),
        }
    }
}

#[test]
fn login_entry_focuses_username() {
    let harness = login_harness(false);
    assert!(
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "Username")
            .is_focused()
    );
}

#[test]
fn login_back_returns_to_overview() {
    let mut harness = login_harness(false);
    harness.get_by_label("← Back to overview").click();
    harness.run_steps(1);

    assert!(matches!(harness.state().page, Page::Overview(_)));
}

#[test]
fn login_back_link_is_centered_below_the_card() {
    let harness = login_harness(false);
    let link = harness.get_by_label("← Back to overview").rect();
    let submit = harness.get_by_label("Sign in").rect();

    assert!((link.center().x - VIEWPORT.x / 2.0).abs() <= 1.0);
    assert!(link.top() - submit.bottom() >= styles::spacing::login_back_link_gap::EGUI_POINTS);
}

#[test]
fn login_back_link_stays_visible_but_disabled_while_authenticating() {
    let mut harness = login_harness(false);
    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let ctx = harness.ctx.clone();
    if let Page::Login(login) = &mut harness.state_mut().page {
        login.login_task.spawn(
            move || {
                task_release.wait();
                Err(ServiceError::Transport(
                    "pending login link test complete".to_owned(),
                ))
            },
            ctx,
        );
    }
    harness.run_steps(1);

    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("← Back to overview") && node.is_disabled()
            })
            .is_some()
    );
    release.wait();
}

#[test]
fn enter_does_not_replace_a_pending_login() {
    let mut harness = login_harness(false);
    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let ctx = harness.ctx.clone();
    if let Page::Login(login) = &mut harness.state_mut().page {
        login.login_task.spawn(
            move || {
                task_release.wait();
                Err(ServiceError::Transport(
                    "pending login test complete".to_owned(),
                ))
            },
            ctx,
        );
    }
    harness.run_steps(1);

    let original_thread = match &harness.state().page {
        Page::Login(PageLogin {
            login_task:
                AsyncState::Pending {
                    handle: Some(handle),
                    ..
                },
            ..
        }) => handle.thread().id(),
        _ => panic!("login task was not pending"),
    };

    harness.key_press(egui::Key::Enter);
    harness.run_steps(1);

    let current_thread = match &harness.state().page {
        Page::Login(PageLogin {
            login_task:
                AsyncState::Pending {
                    handle: Some(handle),
                    ..
                },
            ..
        }) => handle.thread().id(),
        _ => panic!("login task stopped being pending"),
    };
    assert_eq!(current_thread, original_thread);
    release.wait();
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

    let mut focused_submit = login_harness(false);
    focused_submit.get_by_label("Sign in").focus();
    focused_submit.run();
    focused_submit.snapshot("login_focused_submit");
    results.extend_harness(&mut focused_submit);

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
                Err(ServiceError::Transport(
                    "pending login snapshot complete".to_owned(),
                ))
            },
            ctx,
        );
    }
    pending.run_steps(1);
    freeze_time(&mut pending);
    pending.snapshot("login_pending");
    results.extend_harness(&mut pending);
    release.wait();

    let mut focused_disabled = login_harness(false);
    focused_disabled.get_by_label("Sign in").focus();
    focused_disabled.run();
    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let ctx = focused_disabled.ctx.clone();
    if let Page::Login(login) = &mut focused_disabled.state_mut().page {
        login.login_task.spawn(
            move || {
                task_release.wait();
                Err(ServiceError::Transport(
                    "focused disabled login snapshot complete".to_owned(),
                ))
            },
            ctx,
        );
    }
    focused_disabled.run_steps(1);
    freeze_time(&mut focused_disabled);
    focused_disabled.snapshot("login_focused_disabled");
    results.extend_harness(&mut focused_disabled);
    release.wait();

    results.unwrap();
}
