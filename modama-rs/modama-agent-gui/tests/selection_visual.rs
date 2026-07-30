use std::sync::{Arc, Barrier};

use eframe::egui;
use egui_kittest::{kittest::Queryable, Harness, SnapshotResults};
use modama_agent_gui::{
    app::AgentGuiApp,
    pages::{
        select_project::{representative_projects, ProjectSort},
        Page, PageSelectCreateSession, PageSelectProject,
    },
    state::{ActiveSession, Credentials},
    styles,
};

const VIEWPORT: egui::Vec2 = egui::vec2(1024.0, 768.0);
const SESSIONS: [&str; 4] = [
    "2026-06-16-holo-stacks",
    "2026-06-18-holo-stacks",
    "2026-06-20-holo-stacks",
    "2026-06-26-holo-stacks",
];

fn selection_harness(page: Page) -> Harness<'static, AgentGuiApp> {
    selection_harness_with_theme(page, false)
}

fn selection_harness_with_theme(page: Page, dark: bool) -> Harness<'static, AgentGuiApp> {
    let mut harness = Harness::new_eframe(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        styles::theme::install(&cc.egui_ctx);

        let mut app = AgentGuiApp::new();
        app.page = page;
        app.app_state.creds = Some(Credentials {
            username: "representative.user".to_string(),
            password: "representative-password".to_string(),
        });
        app
    });
    harness.set_size(VIEWPORT).set_pixels_per_point(1.0);
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
                representative_projects()
            },
            ctx,
        );
    }
    loading.run_steps(3);
    loading.snapshot("select_project_loading");
    release.wait();
    results.extend_harness(&mut loading);

    let mut unselected_page = PageSelectProject::new();
    unselected_page.available_projects = representative_projects();
    let mut unselected = selection_harness(Page::SelectProject(unselected_page));
    unselected.run_steps(3);
    unselected.snapshot("select_project_unselected");
    results.extend_harness(&mut unselected);

    let mut selected_page = PageSelectProject::new();
    selected_page.available_projects = representative_projects();
    selected_page.selected_project = Some("Holo Stacks".to_string());
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
fn project_search_sort_and_selection_are_accessible() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    assert!(harness
        .query_by_role_and_label(egui::accesskit::Role::TextInput, "Search")
        .is_some());
    assert!(harness
        .query_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
        .is_some());

    harness.get_by_label_contains("Holo Stacks, owner").click();
    harness.run_steps(2);
    harness.get_by_label_contains("Holo Stacks, owner").click();
    harness.run_steps(2);

    let Page::SelectProject(page) = &harness.state().page else {
        panic!("project selection page unexpectedly changed");
    };
    assert_eq!(page.selected_project.as_deref(), Some("Holo Stacks"));
    assert!(harness.query_by_label("Select Project").is_some());
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
                strings(&SESSIONS)
            },
            ctx,
        );
    }
    loading.run_steps(3);
    loading.snapshot("select_session_loading");
    release.wait();
    results.extend_harness(&mut loading);

    let mut unselected_page = PageSelectCreateSession::new("Project Alpha".to_string());
    unselected_page.available_sessions = strings(&SESSIONS);
    let mut unselected = selection_harness(Page::SelectCreateSession(unselected_page));
    unselected.run_steps(3);
    unselected.snapshot("select_session_unselected");
    results.extend_harness(&mut unselected);

    let mut selected_page = PageSelectCreateSession::new("Project Alpha".to_string());
    selected_page.available_sessions = strings(&SESSIONS);
    selected_page.selected_session = Some(SESSIONS[1].to_string());
    let mut selected = selection_harness(Page::SelectCreateSession(selected_page));
    selected.run_steps(3);
    selected.snapshot("select_session_selected");
    results.extend_harness(&mut selected);

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
                ActiveSession {
                    project: "Project Alpha".to_string(),
                    session: SESSIONS[1].to_string(),
                    mount_path: None,
                }
            },
            ctx,
        );
    }
    starting.run_steps(1);
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
                ActiveSession {
                    project: "Project Alpha".to_string(),
                    session: SESSIONS[1].to_string(),
                    mount_path: None,
                }
            },
            ctx,
        );
    }
    harness.run_steps(1);

    assert!(harness.query_by_label("Starting…").is_some());
    harness.get_by_label(SESSIONS[2]).click();
    harness.run_steps(2);

    let Page::SelectCreateSession(page) = &harness.state().page else {
        panic!("session selection page unexpectedly changed");
    };
    assert_eq!(page.selected_session.as_deref(), Some(SESSIONS[1]));
    release.wait();
}
