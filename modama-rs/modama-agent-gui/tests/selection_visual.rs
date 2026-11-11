use std::sync::{Arc, Barrier};

use eframe::egui::{self, accesskit::Role};
use egui_kittest::{Harness, SnapshotResults, kittest::Queryable};
use modama_agent_gui::{
    app::AgentGuiApp,
    async_state::AsyncState,
    guide_screenshots::{VIEWPORT, freeze_time},
    pages::{
        CreateSessionError, Page, PageSelectCreateSession, PageSelectProject,
        select_project::{ProjectSort, representative_projects},
    },
    state::{OperationId, PendingSession, SignedIn},
    styles,
};
use time::{Date, Month};

/// Newest first, as the agent service lists them
const SESSIONS: [&str; 4] = [
    "2026-06-26-holo-stacks",
    "2026-06-20-holo-stacks",
    "2026-06-18-holo-stacks",
    "2026-06-16-holo-stacks",
];

fn session_page_at_fixture_date() -> PageSelectCreateSession {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.session_date =
        Date::from_calendar_date(2026, Month::July, 30).unwrap_or(page.session_date);
    page
}

fn selection_harness(page: Page) -> Harness<'static, AgentGuiApp> {
    selection_harness_with_theme(page, false)
}

fn selection_harness_with_theme(page: Page, dark: bool) -> Harness<'static, AgentGuiApp> {
    selection_harness_at_size(page, dark, VIEWPORT)
}

fn selection_harness_at_size(
    page: Page,
    dark: bool,
    viewport: egui::Vec2,
) -> Harness<'static, AgentGuiApp> {
    let mut harness = Harness::new_eframe(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);

        let mut app = AgentGuiApp::new();
        app.page = page;
        app.app_state.signed_in = Some(SignedIn {
            username: "representative.user".to_string(),
        });
        app
    });
    harness.set_size(viewport).set_pixels_per_point(1.0);
    harness.ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    harness
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn sentinel_wgpu_debug_disabled() {
    let val = std::env::var("WGPU_DEBUG").expect("WGPU_DEBUG should be set");
    assert_eq!(val, "0");
}

#[test]
fn enabled_custom_rows_use_pointing_hand_cursor_and_disabled_actions_do_not() {
    let mut project_page = PageSelectProject::new();
    project_page.available_projects = representative_projects();
    let mut projects = selection_harness(Page::SelectProject(project_page));
    projects.run_steps(3);

    projects.get_by_label_contains("Holo Stacks, owner").hover();
    projects.run_steps(1);
    assert_eq!(
        projects.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );

    projects.remove_cursor();
    projects.run_steps(1);
    projects.get_by_label("Select Project").hover();
    projects.run_steps(1);
    assert_eq!(
        projects.output().platform_output.cursor_icon,
        egui::CursorIcon::Default
    );

    let mut session_page = PageSelectCreateSession::new("Project Alpha".to_string());
    session_page.available_sessions = strings(&SESSIONS);
    let mut sessions = selection_harness(Page::SelectCreateSession(session_page));
    sessions.run_steps(3);

    sessions.get_by_label("Create a new session").hover();
    sessions.run_steps(1);
    assert_eq!(
        sessions.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );

    sessions.remove_cursor();
    sessions.run_steps(1);
    sessions
        .get_by_role_and_label(egui::accesskit::Role::Button, SESSIONS[0])
        .hover();
    sessions.run_steps(1);
    assert_eq!(
        sessions.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
}

#[test]
fn completed_flow_stages_navigate_back() {
    let mut project_page = PageSelectProject::new();
    project_page.available_projects = representative_projects();
    let mut projects = selection_harness(Page::SelectProject(project_page));
    projects.run_steps(3);
    projects.get_by_label("Ingest").click();
    projects.run_steps(1);
    assert!(matches!(projects.state().page, Page::Overview(_)));

    let mut session_page = PageSelectCreateSession::new("Project Alpha".to_string());
    session_page.available_sessions = strings(&SESSIONS);
    let mut sessions = selection_harness(Page::SelectCreateSession(session_page));
    sessions.run_steps(3);
    sessions.get_by_label("Project").click();
    sessions.run_steps(1);
    assert!(matches!(sessions.state().page, Page::SelectProject(_)));
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
fn project_selection_snapshots() {
    let mut results = SnapshotResults::new();

    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let mut loading = selection_harness(Page::SelectProject(PageSelectProject::new()));
    let ctx = loading.ctx.clone();
    if let Page::SelectProject(page) = &mut loading.state_mut().page {
        page.projects_task.spawn(
            move || {
                task_release.wait();
                Ok(representative_projects())
            },
            ctx,
        );
    }
    loading.run_steps(3);
    assert!(
        loading
            .query_by(|node| {
                node.role() == egui::accesskit::Role::TextInput
                    && node.label().as_deref() == Some("Search")
                    && node.is_disabled()
            })
            .is_some()
    );
    assert!(
        loading
            .query_by(|node| {
                node.role() == egui::accesskit::Role::ComboBox
                    && node.label().as_deref() == Some("Sort by")
                    && node.is_disabled()
            })
            .is_some()
    );
    assert!(
        loading
            .query_by_role_and_label(egui::accesskit::Role::ProgressIndicator, "Loading")
            .is_some()
    );
    let progress = loading
        .get_by(|node| {
            node.role() == egui::accesskit::Role::ProgressIndicator && node.label().is_none()
        })
        .rect();
    let loading_label = loading.get_by_label("Fetching projects…").rect();
    let loading_group = progress.union(loading_label);
    let sort = loading
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
        .rect();
    let action = loading.get_by_label("Select Project").rect();
    assert!(
        (loading_group.center().x - VIEWPORT.x / 2.0).abs() <= 1.0,
        "loading center {}, viewport center {}",
        loading_group.center().x,
        VIEWPORT.x / 2.0,
    );
    assert!(
        (loading_group.center().y - (sort.bottom() + action.top()) / 2.0).abs() <= 3.0,
        "loading center {}, content center {}, sort bottom {}, action top {}",
        loading_group.center().y,
        (sort.bottom() + action.top()) / 2.0,
        sort.bottom(),
        action.top(),
    );
    freeze_time(&mut loading);
    loading.snapshot("select_project_loading");
    release.wait();
    results.extend_harness(&mut loading);

    let mut no_access_page = PageSelectProject::new();
    no_access_page.projects_task = AsyncState::Ready {
        payload: Ok(Vec::new()),
    };
    let mut no_access = selection_harness(Page::SelectProject(no_access_page));
    no_access.run_steps(3);
    no_access.snapshot("select_project_no_access");
    results.extend_harness(&mut no_access);

    let mut unselected_page = PageSelectProject::new();
    unselected_page.available_projects = representative_projects();
    let mut unselected = selection_harness(Page::SelectProject(unselected_page));
    unselected.run_steps(3);
    unselected.snapshot("select_project_unselected");
    results.extend_harness(&mut unselected);

    let mut hovered_page = PageSelectProject::new();
    hovered_page.available_projects = representative_projects();
    let mut hovered = selection_harness(Page::SelectProject(hovered_page));
    hovered.run_steps(3);
    let hovered_row = hovered
        .get_by_label_contains("Adaptive Optics, owner")
        .rect();
    hovered.hover_at(egui::pos2(hovered_row.left() + 2.0, hovered_row.center().y));
    hovered.run_steps(2);
    hovered.snapshot("select_project_hovered");
    results.extend_harness(&mut hovered);

    let mut hovered_dark_page = PageSelectProject::new();
    hovered_dark_page.available_projects = representative_projects();
    let mut hovered_dark =
        selection_harness_with_theme(Page::SelectProject(hovered_dark_page), true);
    hovered_dark.run_steps(3);
    let hovered_dark_row = hovered_dark
        .get_by_label_contains("Adaptive Optics, owner")
        .rect();
    hovered_dark.hover_at(egui::pos2(
        hovered_dark_row.left() + 2.0,
        hovered_dark_row.center().y,
    ));
    hovered_dark.run_steps(2);
    hovered_dark.snapshot("select_project_hovered_dark");
    results.extend_harness(&mut hovered_dark);

    let mut sort_open_page = PageSelectProject::new();
    sort_open_page.available_projects = representative_projects();
    let mut sort_open = selection_harness(Page::SelectProject(sort_open_page));
    sort_open.run_steps(3);
    sort_open
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
        .click();
    sort_open.run_steps(2);
    sort_open.remove_cursor();
    sort_open.run_steps(1);
    sort_open.snapshot("select_project_sort_open");
    results.extend_harness(&mut sort_open);

    let mut sort_hovered_page = PageSelectProject::new();
    sort_hovered_page.available_projects = representative_projects();
    let mut sort_hovered = selection_harness(Page::SelectProject(sort_hovered_page));
    sort_hovered.run_steps(3);
    sort_hovered
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
        .click();
    sort_hovered.run_steps(2);
    sort_hovered
        .get_by_role_and_label(egui::accesskit::Role::Button, "Name A-Z")
        .hover();
    sort_hovered.run_steps(2);
    sort_hovered.snapshot("select_project_sort_item_hovered");
    results.extend_harness(&mut sort_hovered);

    let mut selected_page = PageSelectProject::new();
    selected_page.available_projects = representative_projects();
    selected_page.selected_project = Some("Adaptive Optics".to_string());
    let mut selected = selection_harness(Page::SelectProject(selected_page));
    selected.run_steps(3);
    selected.snapshot("select_project_selected");
    results.extend_harness(&mut selected);

    let mut substring_page = PageSelectProject::new();
    substring_page.available_projects = representative_projects();
    substring_page.search_query = "holo".to_string();
    let mut substring = selection_harness(Page::SelectProject(substring_page));
    substring.run_steps(3);
    substring.snapshot("select_project_search_substring");
    results.extend_harness(&mut substring);

    let mut fuzzy_page = PageSelectProject::new();
    fuzzy_page.available_projects = representative_projects();
    fuzzy_page.search_query = "hollo staks".to_string();
    let mut fuzzy = selection_harness(Page::SelectProject(fuzzy_page));
    fuzzy.run_steps(3);
    fuzzy.snapshot("select_project_search_fuzzy");
    results.extend_harness(&mut fuzzy);

    let mut empty_page = PageSelectProject::new();
    empty_page.available_projects = representative_projects();
    empty_page.search_query = "zzzzzz".to_string();
    let mut empty = selection_harness(Page::SelectProject(empty_page));
    empty.run_steps(3);
    empty.snapshot("select_project_search_empty");
    results.extend_harness(&mut empty);

    let mut name_sorted_page = PageSelectProject::new();
    name_sorted_page.available_projects = representative_projects();
    name_sorted_page.sort = ProjectSort::Name;
    let mut name_sorted = selection_harness(Page::SelectProject(name_sorted_page));
    name_sorted.run_steps(3);
    let first_project = name_sorted.get_by_label_contains("Adaptive Optics, owner");
    for _ in 0..8 {
        first_project.scroll_down();
    }
    name_sorted.run_steps(2);
    name_sorted.snapshot("select_project_scrolled");
    results.extend_harness(&mut name_sorted);

    let mut dark_page = PageSelectProject::new();
    dark_page.available_projects = representative_projects();
    dark_page.selected_project = Some("Holo Stacks".to_string());
    let mut dark = selection_harness_with_theme(Page::SelectProject(dark_page), true);
    dark.run_steps(3);
    dark.snapshot("select_project_selected_dark");
    results.extend_harness(&mut dark);

    results.unwrap();
}

#[test]
fn project_sort_dropdown_options_are_accessible_and_selectable() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    harness
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
        .click();
    harness.run_steps(2);

    assert!(
        harness
            .query_by(|node| {
                node.role() == egui::accesskit::Role::Button
                    && node.label().as_deref() == Some("Last modified")
                    && node.toggled() == Some(egui::accesskit::Toggled::True)
            })
            .is_some()
    );
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Name A-Z")
        .click();
    harness.run_steps(2);

    let Page::SelectProject(page) = &harness.state().page else {
        panic!("project selection page unexpectedly changed");
    };
    assert_eq!(page.sort, ProjectSort::Name);
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::Button, "Name A-Z")
            .is_none()
    );
    assert!(
        harness
            .get_by_label_contains("Adaptive Optics, owner")
            .rect()
            .top()
            < harness
                .get_by_label_contains("Holo Stacks, owner")
                .rect()
                .top()
    );
}

#[test]
fn an_empty_project_fetch_stays_in_the_no_access_state() {
    let mut page = PageSelectProject::new();
    page.projects_task = AsyncState::Ready {
        payload: Ok(Vec::new()),
    };
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    assert!(harness.query_by_label("No projects available").is_some());
    assert!(
        harness
            .query_by_label("You don’t have access to any projects.")
            .is_some()
    );
    assert!(
        harness
            .query_by_role(egui::accesskit::Role::ProgressIndicator)
            .is_none()
    );

    harness.run_steps(3);
    let Page::SelectProject(page) = &harness.state().page else {
        panic!("project selection page unexpectedly changed");
    };
    assert!(matches!(page.projects_task, AsyncState::Completed));
    assert!(!page.is_busy());
}

#[test]
fn project_search_sort_and_selection_are_accessible() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::TextInput, "Search")
            .is_some()
    );
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
            .is_some()
    );

    let holo_row = harness.get_by_label_contains("Holo Stacks, owner").rect();
    let edge_click = egui::pos2(holo_row.left() + 2.0, holo_row.center().y);
    harness.hover_at(edge_click);
    harness.drag_at(edge_click);
    harness.drop_at(edge_click);
    harness.run_steps(2);
    harness.get_by_label_contains("Holo Stacks, owner").click();
    harness.run_steps(2);

    let Page::SelectProject(page) = &harness.state().page else {
        panic!("project selection page unexpectedly changed");
    };
    assert_eq!(page.selected_project.as_deref(), Some("Holo Stacks"));
    assert!(
        harness
            .get_by_label_contains("Holo Stacks, owner")
            .is_focused()
    );
    assert!(harness.query_by_label("Select Project").is_some());
    assert!(
        harness
            .query_by_role(egui::accesskit::Role::RadioButton)
            .is_none()
    );
    assert!(
        harness
            .query_by(|node| {
                node.label()
                    .is_some_and(|label| label.starts_with("Holo Stacks, owner"))
                    && node.toggled() == Some(egui::accesskit::Toggled::True)
            })
            .is_some()
    );
    assert!(
        harness
            .query_all_by_label("1 additional member")
            .next()
            .is_some()
    );
    assert!(
        harness
            .query_all_by_label("2 additional members")
            .next()
            .is_some()
    );
}

#[test]
fn project_page_tab_order_follows_visual_order() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    assert!(harness.get_by_label("Search").is_focused());
    press_tab(&mut harness, false);
    assert!(harness.get_by_label("Sort by").is_focused());

    press_tab(&mut harness, false);
    assert!(
        harness
            .get_by_label_contains("Holo Stacks, owner")
            .is_focused()
    );

    press_tab(&mut harness, true);
    assert!(harness.get_by_label("Sort by").is_focused());
}

#[test]
fn project_list_focus_scrolls_to_selected_offscreen_item() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    page.selected_project = Some("Holo Stacks Archive".to_owned());
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    assert!(harness.get_by_label("Search").is_focused());
    press_tab(&mut harness, false);
    press_tab(&mut harness, false);
    harness.run_steps(2);

    let selected = harness.get_by_label_contains("Holo Stacks Archive, owner");
    assert!(selected.is_focused());
    let selected_rect = selected.rect();
    let toolbar_bottom = harness.get_by_label("Sort by").rect().bottom();
    let action_top = harness.get_by_label("Select Project").rect().top();
    assert!(
        selected_rect.top() >= toolbar_bottom && selected_rect.bottom() <= action_top,
        "selected project should be scrolled into the list viewport: {selected_rect:?}, toolbar bottom {toolbar_bottom}, action top {action_top}"
    );
}

#[test]
fn project_list_focus_tracks_selection_after_clearing_search() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    harness.get_by_label("Search").type_text("wideband");
    harness.run_steps(2);
    press_tab(&mut harness, false);
    press_tab(&mut harness, false);
    harness.key_press(egui::Key::Space);
    harness.run_steps(2);

    assert!(
        harness
            .get_by_label_contains("Wideband Receiver, owner")
            .is_focused()
    );
    harness.get_by_label("Search").focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::A);
    harness.key_press(egui::Key::Backspace);
    harness.run_steps(2);
    press_tab(&mut harness, false);
    press_tab(&mut harness, false);
    harness.run_steps(2);

    assert!(
        harness
            .get_by_label_contains("Wideband Receiver, owner")
            .is_focused()
    );
}

#[test]
fn project_list_keeps_focus_after_scrolling_selected_archive_into_view() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    harness.get_by_label("Search").type_text("archive");
    harness.run_steps(4);
    press_tab(&mut harness, false);
    press_tab(&mut harness, false);
    harness.key_press(egui::Key::Space);
    harness.run_steps(4);

    harness.get_by_label("Search").focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::A);
    harness.key_press(egui::Key::Backspace);
    harness.run_steps(4);
    press_tab(&mut harness, false);
    press_tab(&mut harness, false);
    harness.run_steps(4);

    for _ in 0..5 {
        assert!(
            harness
                .get_by_label_contains("Holo Stacks Archive, owner")
                .is_focused()
        );
        harness.run();
    }
    press_tab(&mut harness, false);
    assert!(harness.get_by_label("Select Project").is_focused());
    press_tab(&mut harness, true);
    assert!(
        harness
            .get_by_label_contains("Holo Stacks Archive, owner")
            .is_focused()
    );

    harness.get_by_label("Search").focus();
    harness.run();
    press_tab(&mut harness, false);
    press_tab(&mut harness, false);
    harness.run_steps(5);
    assert!(
        harness
            .get_by_label_contains("Holo Stacks Archive, owner")
            .is_focused()
    );
    press_tab(&mut harness, false);
    assert!(harness.get_by_label("Select Project").is_focused());
}

#[test]
fn session_page_tab_order_follows_visual_order() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_owned());
    page.available_sessions = strings(&SESSIONS);
    page.selected_session = Some(SESSIONS[0].to_owned());
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);

    harness.get_by_label("Create a new session").focus();
    harness.run();
    assert!(harness.get_by_label("Create a new session").is_focused());
    press_tab(&mut harness, false);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, SESSIONS[0])
            .is_focused()
    );

    harness
        .get_by_role_and_label(Role::Button, SESSIONS[0])
        .focus();
    harness.run();
    assert!(
        harness
            .get_by_role_and_label(Role::Button, SESSIONS[0])
            .is_focused()
    );
    press_tab(&mut harness, false);
    assert!(harness.get_by_label("Select Session").is_focused());

    press_tab(&mut harness, true);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, SESSIONS[0])
            .is_focused()
    );
}

#[test]
fn project_list_arrows_wrap_and_move_selection() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    harness.get_by_label_contains("Holo Stacks, owner").focus();
    harness.run();
    harness.key_press(egui::Key::ArrowUp);
    harness.run_steps(2);

    let last = harness.get_by_label_contains("Holo Stacks Archive, owner");
    assert!(last.is_focused());
    if let Page::SelectProject(page) = &harness.state().page {
        assert_eq!(
            page.selected_project.as_deref(),
            Some("Holo Stacks Archive")
        );
    }
}

#[test]
fn project_list_arrows_move_down_and_back_up_with_focus() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    let names = [
        "Holo Stacks",
        "Adaptive Optics",
        "Neutrino Runs",
        "Exoplanet Atmospheres",
    ];
    harness.get_by_label_contains("Holo Stacks, owner").focus();
    harness.run();

    for name in &names[1..] {
        harness.key_press(egui::Key::ArrowDown);
        harness.run_steps(2);
        let label = format!("{name}, owner");
        assert!(
            harness.get_by_label_contains(&label).is_focused(),
            "expected {name:?} to retain focus after ArrowDown"
        );
        if let Page::SelectProject(page) = &harness.state().page {
            assert_eq!(page.selected_project.as_deref(), Some(*name));
        }
    }

    for name in names[..3].iter().rev() {
        harness.key_press(egui::Key::ArrowUp);
        harness.run_steps(2);
        let label = format!("{name}, owner");
        assert!(
            harness.get_by_label_contains(&label).is_focused(),
            "expected {name:?} to regain focus after ArrowUp"
        );
        if let Page::SelectProject(page) = &harness.state().page {
            assert_eq!(page.selected_project.as_deref(), Some(*name));
        }
    }
}

#[test]
fn project_list_arrow_down_keeps_focus_while_scrolling() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    harness.get_by_label_contains("Holo Stacks, owner").focus();
    harness.run();
    for _ in 0..12 {
        harness.key_press(egui::Key::ArrowDown);
        harness.run_steps(2);
        assert!(
            harness
                .query_by(|node| {
                    node.is_focused() && node.label().is_some_and(|label| label.contains(", owner"))
                })
                .is_some()
        );
    }
}

#[test]
fn session_list_arrows_wrap_and_move_selection() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_owned());
    page.available_sessions = strings(&SESSIONS);
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);

    let first = harness.get_by_role_and_label(Role::Button, SESSIONS[0]);
    first.focus();
    harness.run();
    harness.key_press(egui::Key::ArrowUp);
    harness.run_steps(2);

    let last = harness.get_by_role_and_label(Role::Button, SESSIONS[3]);
    assert!(last.is_focused());
    if let Page::SelectCreateSession(page) = &harness.state().page {
        assert_eq!(page.selected_session.as_deref(), Some(SESSIONS[3]));
    }
}

#[test]
fn session_command_n_enters_create_mode_and_escape_returns_to_list() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_owned());
    page.available_sessions = strings(&SESSIONS);
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    harness.run_steps(2);
    assert!(
        harness
            .get_by_role_and_label(Role::TextInput, "Session name")
            .is_focused()
    );

    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.get_by_label("Create a new session").is_focused());
}

#[test]
fn selected_project_must_be_visible_to_continue() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    page.selected_project = Some("Holo Stacks".to_string());
    page.search_query = "aurora".to_string();
    let harness = selection_harness(Page::SelectProject(page));

    assert!(harness
        .query_by(|node| node.label().as_deref() == Some("Select Project") && node.is_disabled())
        .is_some());
}

#[test]
fn project_toolbar_and_dynamic_list_follow_available_geometry() {
    let mut list_spans = Vec::new();
    let mut bottom_margins = Vec::new();

    for height in [600.0, 768.0, 900.0] {
        let mut page = PageSelectProject::new();
        page.available_projects = representative_projects();
        let mut harness = selection_harness_at_size(
            Page::SelectProject(page),
            false,
            egui::vec2(VIEWPORT.x, height),
        );
        harness.run_steps(3);

        let search = harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "Search")
            .rect();
        let sort = harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
            .rect();
        assert!((search.top() - sort.top()).abs() <= 1.0);
        assert!((search.height() - sort.height()).abs() <= 1.0);

        let search_label = harness
            .get_by(|node| {
                node.role() == egui::accesskit::Role::Label
                    && node.value().as_deref() == Some("Search")
            })
            .rect();
        let sort_label = harness
            .get_by(|node| {
                node.role() == egui::accesskit::Role::Label
                    && node.value().as_deref() == Some("Sort by")
            })
            .rect();
        assert!((search_label.top() - sort_label.top()).abs() <= 1.0);
        let expected_label_gap = styles::spacing::field_label_gap::EGUI_POINTS;
        assert!((search.top() - search_label.bottom() - expected_label_gap).abs() <= 1.0);
        assert!((sort.top() - sort_label.bottom() - expected_label_gap).abs() <= 1.0);

        let first_row = harness.get_by_label_contains("Holo Stacks, owner").rect();
        let second_row = harness
            .get_by_label_contains("Adaptive Optics, owner")
            .rect();
        assert!(
            (second_row.top() - first_row.bottom()).abs() <= 1.0,
            "consecutive project rows must meet at the separator: first={first_row:?}, second={second_row:?}"
        );
        let action = harness.get_by_label("Select Project").rect();
        let guide = harness.get_by_label("go to Ingest").rect().left();
        let flow_overview = harness.get_by_label("Ingest").rect();
        let heading = harness.get_by_label("Select a project").rect();
        let card_outer_left = styles::outer_guide_inset();
        let card_border = styles::components::standard_border::width::EGUI_POINTS;
        assert!((heading.left() - guide).abs() <= 1.0);
        assert!((flow_overview.left() - guide).abs() <= 1.0);
        assert!(flow_overview.bottom() <= heading.top());
        assert!((search_label.left() - guide).abs() <= 1.0);
        assert!((search.left() - card_outer_left).abs() <= 1.0);
        assert!(
            (search.left() + card_border + styles::spacing::button_padding_x::EGUI_POINTS - guide)
                .abs()
                <= 1.0
        );
        assert!(
            (first_row.left() + styles::spacing::button_padding_x::EGUI_POINTS - guide).abs()
                <= 1.0
        );
        assert!((first_row.left() - (card_outer_left + card_border)).abs() <= 1.0);
        assert!((first_row.right() - (VIEWPORT.x - card_outer_left - card_border)).abs() <= 1.0);
        assert!(
            (action.left() + styles::spacing::button_padding_x::EGUI_POINTS - guide).abs() <= 1.0
        );

        let dust_row = harness
            .get_by_label_contains("Dust Polarization, owner")
            .rect();
        let overflow = harness
            .get_all_by_label("1 additional member")
            .into_iter()
            .find(|node| dust_row.contains(node.rect().center()))
            .expect("Dust Polarization overflow avatar")
            .rect();
        assert!(
            (overflow.right()
                - (first_row.right() - styles::spacing::button_padding_x::EGUI_POINTS - 126.0))
                .abs()
                <= 1.0
        );

        list_spans.push(action.top() - first_row.top());
        bottom_margins.push(height - action.bottom());
    }

    assert!(list_spans[0] < list_spans[1]);
    assert!(list_spans[1] < list_spans[2]);
    for margin in &bottom_margins {
        assert!(*margin >= styles::spacing::form_field_gap::EGUI_POINTS);
        assert!((*margin - bottom_margins[0]).abs() <= 1.0);
    }
}

#[test]
fn session_selection_snapshots() {
    let mut results = SnapshotResults::new();

    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let mut loading = selection_harness(Page::SelectCreateSession(PageSelectCreateSession::new(
        "Project Alpha".to_string(),
    )));
    let ctx = loading.ctx.clone();
    if let Page::SelectCreateSession(page) = &mut loading.state_mut().page {
        page.sessions_task.spawn(
            move || {
                task_release.wait();
                Ok(strings(&SESSIONS))
            },
            ctx,
        );
    }
    loading.run_steps(3);
    freeze_time(&mut loading);
    loading.snapshot("select_session_loading");
    release.wait();
    results.extend_harness(&mut loading);

    let mut empty_page = PageSelectCreateSession::new("Project Alpha".to_string());
    empty_page.sessions_task = AsyncState::Ready {
        payload: Ok(Vec::new()),
    };
    let mut empty = selection_harness(Page::SelectCreateSession(empty_page));
    empty.run_steps(3);
    empty.snapshot("select_session_empty");
    empty.run_steps(3);
    if let Page::SelectCreateSession(page) = &empty.state().page {
        assert!(matches!(page.sessions_task, AsyncState::Completed));
        assert!(!page.is_busy());
    } else {
        panic!("session selection page unexpectedly changed");
    }
    results.extend_harness(&mut empty);

    let mut unselected_page = PageSelectCreateSession::new("Project Alpha".to_string());
    unselected_page.available_sessions = strings(&SESSIONS);
    let mut unselected = selection_harness(Page::SelectCreateSession(unselected_page));
    unselected.run_steps(3);
    unselected.snapshot("select_session_unselected");
    results.extend_harness(&mut unselected);

    let mut create_hovered_page = PageSelectCreateSession::new("Project Alpha".to_string());
    create_hovered_page.available_sessions = strings(&SESSIONS);
    let mut create_hovered = selection_harness(Page::SelectCreateSession(create_hovered_page));
    create_hovered.run_steps(3);
    create_hovered.get_by_label("Create a new session").hover();
    create_hovered.run_steps(2);
    create_hovered.snapshot("select_session_create_hovered");
    results.extend_harness(&mut create_hovered);

    let mut existing_hovered_page = PageSelectCreateSession::new("Project Alpha".to_string());
    existing_hovered_page.available_sessions = strings(&SESSIONS);
    let mut existing_hovered = selection_harness(Page::SelectCreateSession(existing_hovered_page));
    existing_hovered.run_steps(3);
    existing_hovered
        .get_by_role_and_label(Role::Button, SESSIONS[1])
        .hover();

    existing_hovered.run_steps(2);
    existing_hovered.snapshot("select_session_existing_hovered");
    results.extend_harness(&mut existing_hovered);

    let mut selected_page = PageSelectCreateSession::new("Project Alpha".to_string());
    selected_page.available_sessions = strings(&SESSIONS);
    selected_page.selected_session = Some(SESSIONS[1].to_string());
    let mut selected = selection_harness(Page::SelectCreateSession(selected_page));
    selected.run_steps(3);
    selected.snapshot("select_session_selected");
    results.extend_harness(&mut selected);

    let mut dark_page = PageSelectCreateSession::new("Project Alpha".to_string());
    dark_page.available_sessions = strings(&SESSIONS);
    dark_page.selected_session = Some(SESSIONS[1].to_string());
    let mut dark = selection_harness_with_theme(Page::SelectCreateSession(dark_page), true);
    dark.run_steps(3);
    dark.snapshot("select_session_selected_dark");
    results.extend_harness(&mut dark);

    let mut form_page = session_page_at_fixture_date();
    form_page.available_sessions = strings(&SESSIONS);
    form_page.new_session_input = "2026-07-30-".to_owned();
    let mut form = selection_harness(Page::SelectCreateSession(form_page));
    form.run_steps(3);
    form.get_by_label("Create a new session").click();
    form.run_steps(2);
    form.snapshot("select_session_create_form");
    results.extend_harness(&mut form);

    let mut validation_page = session_page_at_fixture_date();
    validation_page.available_sessions = strings(&SESSIONS);
    validation_page.new_session_input = "2026-07-30/invalid".to_owned();
    let mut validation = selection_harness(Page::SelectCreateSession(validation_page));
    validation.run_steps(3);
    validation.get_by_label("Create a new session").click();
    validation.run_steps(2);
    validation.get_by_label("Create Session").click();
    validation.run_steps(2);
    validation.snapshot("select_session_create_validation");
    results.extend_harness(&mut validation);

    let mut existing_page = session_page_at_fixture_date();
    existing_page.available_sessions = strings(&SESSIONS);
    existing_page.new_session_input = SESSIONS[1].to_owned();
    let mut existing = selection_harness(Page::SelectCreateSession(existing_page));
    existing.run_steps(3);
    existing.get_by_label("Create a new session").click();
    existing.run_steps(2);
    existing.get_by_label("Create Session").click();
    existing.run_steps(3);
    existing.snapshot("select_session_create_existing");
    results.extend_harness(&mut existing);

    let mut rejected_page = session_page_at_fixture_date();
    rejected_page.available_sessions = strings(&SESSIONS);
    rejected_page.new_session_input = "2026-07-30-forbidden-run".to_owned();
    let mut rejected = selection_harness(Page::SelectCreateSession(rejected_page));
    rejected.run_steps(3);
    rejected.get_by_label("Create a new session").click();
    rejected.run_steps(2);
    let operation_id = OperationId::next();
    let app = rejected.state_mut();
    app.app_state.begin_start(
        operation_id,
        "Project Alpha".to_owned(),
        "2026-07-30-forbidden-run".to_owned(),
    );
    if let Page::SelectCreateSession(page) = &mut app.page {
        page.start_operation_id = Some(operation_id);
        page.create_session_task = AsyncState::Ready {
            payload: Err(CreateSessionError::Server(
                "Session name was rejected by the server.".to_owned(),
            )),
        };
    }
    rejected.run_steps(2);
    rejected.snapshot("select_session_create_rejected");
    results.extend_harness(&mut rejected);

    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let mut creating_page = session_page_at_fixture_date();
    creating_page.available_sessions = strings(&SESSIONS);
    creating_page.new_session_input = "2026-07-30-alignment-run".to_owned();
    let mut creating = selection_harness(Page::SelectCreateSession(creating_page));
    creating.run_steps(3);
    creating.get_by_label("Create a new session").click();
    creating.run_steps(2);
    let ctx = creating.ctx.clone();
    if let Page::SelectCreateSession(page) = &mut creating.state_mut().page {
        page.create_session_task.spawn(
            move || {
                task_release.wait();
                Err(CreateSessionError::Server(
                    "pending create snapshot complete".to_owned(),
                ))
            },
            ctx,
        );
    }
    creating.run_steps(1);
    freeze_time(&mut creating);
    creating.snapshot("select_session_creating");
    release.wait();
    results.extend_harness(&mut creating);

    let mut scrolled_page = PageSelectCreateSession::new("Project Alpha".to_string());
    scrolled_page.available_sessions = (1..=24)
        .rev()
        .map(|day| format!("2026-06-{day:02}-holo-stacks"))
        .collect();
    let mut scrolled = selection_harness(Page::SelectCreateSession(scrolled_page));
    scrolled.run_steps(3);
    let first_session = scrolled.get_by_role_and_label(Role::Button, "2026-06-24-holo-stacks");
    for _ in 0..8 {
        first_session.scroll_down();
    }
    scrolled.run_steps(2);
    scrolled.snapshot("select_session_scrolled");
    results.extend_harness(&mut scrolled);

    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let mut starting_page = PageSelectCreateSession::new("Project Alpha".to_string());
    starting_page.available_sessions = strings(&SESSIONS);
    starting_page.selected_session = Some(SESSIONS[1].to_string());
    let mut starting = selection_harness(Page::SelectCreateSession(starting_page));
    let ctx = starting.ctx.clone();
    if let Page::SelectCreateSession(page) = &mut starting.state_mut().page {
        page.create_ingest_task.spawn(
            move || {
                task_release.wait();
                Ok(PendingSession {
                    share_id: "starting-share".to_owned(),
                    project: "Project Alpha".to_string(),
                    session: SESSIONS[1].to_string(),
                    created_by: None,
                })
            },
            ctx,
        );
    }
    starting.run_steps(1);
    freeze_time(&mut starting);
    starting.snapshot("select_session_starting");
    release.wait();
    results.extend_harness(&mut starting);

    results.unwrap();
}

#[test]
fn session_choices_are_disabled_while_starting() {
    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.available_sessions = strings(&SESSIONS);
    page.selected_session = Some(SESSIONS[1].to_string());
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    let ctx = harness.ctx.clone();
    if let Page::SelectCreateSession(page) = &mut harness.state_mut().page {
        page.create_ingest_task.spawn(
            move || {
                task_release.wait();
                Ok(PendingSession {
                    share_id: "starting-share".to_owned(),
                    project: "Project Alpha".to_string(),
                    session: SESSIONS[1].to_string(),
                    created_by: None,
                })
            },
            ctx,
        );
    }
    harness.run_steps(1);

    assert!(harness.query_by_label("Starting…").is_some());
    harness
        .get_by_role_and_label(Role::Button, SESSIONS[2])
        .click();

    harness.run_steps(2);

    let Page::SelectCreateSession(page) = &harness.state().page else {
        panic!("session selection page unexpectedly changed");
    };
    assert_eq!(page.selected_session.as_deref(), Some(SESSIONS[1]));
    release.wait();
}

#[test]
fn session_selection_and_creation_controls_are_accessible() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.available_sessions = strings(&SESSIONS);
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);

    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::Button, "Create a new session")
            .is_some()
    );
    assert!(
        harness
            .query_by_role(egui::accesskit::Role::RadioButton)
            .is_none()
    );

    harness
        .get_by_role_and_label(Role::Button, SESSIONS[1])
        .click();

    harness.run_steps(2);
    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some(SESSIONS[1])
                    && node.toggled() == Some(egui::accesskit::Toggled::True)
            })
            .is_some()
    );
    assert!(
        harness
            .get_by_role_and_label(Role::Button, SESSIONS[1])
            .is_focused()
    );

    harness.get_by_label("Create a new session").click();
    harness.run_steps(2);
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::TextInput, "Session name")
            .is_some()
    );
    harness.get_by_label("Cancel").click();
    harness.run_steps(2);

    let Page::SelectCreateSession(page) = &harness.state().page else {
        panic!("session selection page unexpectedly changed");
    };
    assert_eq!(page.selected_session.as_deref(), Some(SESSIONS[1]));
    assert!(harness.query_by_label("Create a new session").is_some());
}

#[test]
fn enter_submits_the_create_session_form() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.available_sessions = strings(&SESSIONS);
    page.new_session_input = "2026-07-30-enter-run".to_owned();
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);
    harness.get_by_label("Create a new session").click();
    harness.run_steps(2);

    let input = harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Session name");
    input.focus();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run_steps(1);

    match &harness.state().page {
        Page::SelectCreateSession(page) => assert!(page.create_session_task.is_pending()),
        Page::Overview(_) => {}
        _ => panic!("session creation changed to an unexpected page"),
    }
}

#[test]
fn creating_an_existing_session_asks_first_and_cancel_keeps_the_name() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.available_sessions = strings(&SESSIONS);
    page.new_session_input = format!(" {} ", SESSIONS[1]);
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);
    harness.get_by_label("Create a new session").click();
    harness.run_steps(2);
    harness.get_by_label("Create Session").click();
    harness.run_steps(3);

    let message = format!(
        "The session {} exists already in project Project Alpha. Start the ingest into it?",
        SESSIONS[1]
    );
    harness.get_by_label(&message);
    match &harness.state().page {
        Page::SelectCreateSession(page) => assert!(!page.create_session_task.is_pending()),
        _ => panic!("the confirmation left the session page"),
    }

    // the form's Cancel is still in the tree, behind the dialog's
    let cancels: Vec<_> = harness
        .query_all_by_role_and_label(Role::Button, "Cancel")
        .collect();
    assert_eq!(cancels.len(), 2);
    cancels[1].click();
    harness.run_steps(3);
    assert!(harness.query_by_label(&message).is_none());
    match &harness.state().page {
        Page::SelectCreateSession(page) => {
            assert!(!page.create_session_task.is_pending());
            assert_eq!(page.new_session_input, format!(" {} ", SESSIONS[1]));
        }
        _ => panic!("cancelling left the session page"),
    }
    harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Session name");
}

#[test]
fn confirming_an_existing_session_starts_the_ingest_into_it() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.available_sessions = strings(&SESSIONS);
    page.new_session_input = SESSIONS[1].to_owned();
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);
    harness.get_by_label("Create a new session").click();
    harness.run_steps(2);
    harness.get_by_label("Create Session").click();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::Button, "Use existing session")
        .click();
    harness.run_steps(1);

    match &harness.state().page {
        Page::SelectCreateSession(page) => assert!(page.create_session_task.is_pending()),
        Page::Overview(_) => {}
        _ => panic!("confirming changed to an unexpected page"),
    }
}

#[test]
fn a_new_session_name_is_created_without_asking() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.available_sessions = strings(&SESSIONS);
    page.new_session_input = "2026-07-30-new-run".to_owned();
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);
    harness.get_by_label("Create a new session").click();
    harness.run_steps(2);
    harness.get_by_label("Create Session").click();
    harness.run_steps(1);

    assert!(harness.query_by_label("Session exists already").is_none());
    match &harness.state().page {
        Page::SelectCreateSession(page) => assert!(page.create_session_task.is_pending()),
        Page::Overview(_) => {}
        _ => panic!("session creation changed to an unexpected page"),
    }
}

#[test]
fn asynchronous_session_creation_errors_remain_on_the_form() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.available_sessions = strings(&SESSIONS);
    page.new_session_input = "2026-07-30-forbidden-run".to_owned();
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);
    harness.get_by_label("Create a new session").click();
    harness.run_steps(2);

    let operation_id = OperationId::next();
    let app = harness.state_mut();
    app.app_state.begin_start(
        operation_id,
        "Project Alpha".to_owned(),
        "2026-07-30-forbidden-run".to_owned(),
    );
    if let Page::SelectCreateSession(page) = &mut app.page {
        page.start_operation_id = Some(operation_id);
        page.create_session_task = AsyncState::Ready {
            payload: Err(CreateSessionError::Server(
                "Session name was rejected by the server.".to_owned(),
            )),
        };
    }
    harness.run_steps(2);

    assert!(
        harness
            .query_by_label("Session name was rejected by the server.")
            .is_some()
    );
    assert!(harness.query_by_label("Create Session").is_some());
    assert!(harness.query_by_label("Cancel").is_some());
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::TextInput, "Session name")
            .is_some()
    );
    let Page::SelectCreateSession(page) = &harness.state().page else {
        panic!("session creation unexpectedly changed the page");
    };
    assert_eq!(page.new_session_input, "2026-07-30-forbidden-run");
    assert!(!page.create_ingest_task.is_pending());
}

#[test]
fn successful_session_creation_returns_to_overview_while_waiting_for_the_agent() {
    let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
    page.available_sessions = strings(&SESSIONS);
    page.new_session_input = "2026-07-30-new-run".to_owned();
    let mut harness = selection_harness(Page::SelectCreateSession(page));
    harness.run_steps(3);
    harness.get_by_label("Create a new session").click();
    harness.run_steps(2);

    let operation_id = OperationId::next();
    let app = harness.state_mut();
    app.app_state.begin_start(
        operation_id,
        "Project Alpha".to_owned(),
        "2026-07-30-new-run".to_owned(),
    );
    if let Page::SelectCreateSession(page) = &mut app.page {
        page.start_operation_id = Some(operation_id);
        page.create_session_task = AsyncState::Ready {
            payload: Ok(PendingSession {
                share_id: "created-share".to_owned(),
                project: "Project Alpha".to_owned(),
                session: "2026-07-30-new-run".to_owned(),
                created_by: None,
            }),
        };
    }
    harness.run_steps(2);

    assert!(matches!(harness.state().page, Page::Overview(_)));
    let pending = harness
        .state()
        .app_state
        .pending_start()
        .expect("successful creation should wait for the agent");
    assert_eq!(pending.project, "Project Alpha");
    assert_eq!(pending.session, "2026-07-30-new-run");
}

#[test]
fn session_list_follows_available_geometry_and_shared_guide() {
    let mut list_spans = Vec::new();
    let mut bottom_margins = Vec::new();

    for height in [600.0, 768.0, 900.0] {
        let mut page = PageSelectCreateSession::new("Project Alpha".to_string());
        page.available_sessions = (1..=24)
            .rev()
            .map(|day| format!("2026-06-{day:02}-holo-stacks"))
            .collect();
        let mut harness = selection_harness_at_size(
            Page::SelectCreateSession(page),
            false,
            egui::vec2(VIEWPORT.x, height),
        );
        harness.run_steps(3);

        let guide = harness.get_by_label("go to Ingest").rect().left();
        let flow_overview = harness.get_by_label("Ingest").rect();
        let heading = harness
            .get_by_label("Select or create a session in project 'Project Alpha'")
            .rect();
        let create_row = harness.get_by_label("Create a new session").rect();
        harness
            .get_by_role_and_label(Role::Button, "2026-06-24-holo-stacks")
            .rect();
        let action = harness.get_by_label("Select Session").rect();
        let card_border = styles::components::standard_border::width::EGUI_POINTS;
        let card_outer_left = styles::outer_guide_inset();

        assert!((heading.left() - guide).abs() <= 1.0);
        assert!((flow_overview.left() - guide).abs() <= 1.0);
        assert!(flow_overview.bottom() <= heading.top());
        assert!(
            (create_row.left() + styles::spacing::button_padding_x::EGUI_POINTS - guide).abs()
                <= 1.0
        );
        assert!((create_row.left() - (card_outer_left + card_border)).abs() <= 1.0);
        assert!((create_row.right() - (VIEWPORT.x - card_outer_left - card_border)).abs() <= 1.0);
        assert!(
            (action.left() + styles::spacing::button_padding_x::EGUI_POINTS - guide).abs() <= 1.0
        );

        list_spans.push(action.top() - create_row.top());
        bottom_margins.push(height - action.bottom());
    }

    assert!(list_spans[0] < list_spans[1]);
    assert!(list_spans[1] < list_spans[2]);
    for margin in &bottom_margins {
        assert!(*margin >= styles::spacing::form_field_gap::EGUI_POINTS);
        assert!((*margin - bottom_margins[0]).abs() <= 1.0);
    }
}

#[test]
fn session_loading_is_centered_in_the_list_area() {
    let release = Arc::new(Barrier::new(2));
    let task_release = Arc::clone(&release);
    let mut harness = selection_harness(Page::SelectCreateSession(PageSelectCreateSession::new(
        "Project Alpha".to_owned(),
    )));
    let ctx = harness.ctx.clone();
    if let Page::SelectCreateSession(page) = &mut harness.state_mut().page {
        page.sessions_task.spawn(
            move || {
                task_release.wait();
                Ok(strings(&SESSIONS))
            },
            ctx,
        );
    }
    harness.run_steps(3);

    let progress = harness
        .get_by(|node| {
            node.role() == egui::accesskit::Role::ProgressIndicator && node.label().is_none()
        })
        .rect();
    let label = harness.get_by_label("Fetching sessions…").rect();
    let group = progress.union(label);
    let heading = harness
        .get_by_label("Select or create a session in project 'Project Alpha'")
        .rect();
    let action = harness.get_by_label("Select Session").rect();
    assert!((group.center().x - VIEWPORT.x / 2.0).abs() <= 1.0);
    assert!(group.center().y > heading.bottom() && group.center().y < action.top());
    release.wait();
}
