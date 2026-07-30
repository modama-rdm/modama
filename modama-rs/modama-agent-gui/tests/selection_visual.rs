use std::sync::{Arc, Barrier};

use eframe::egui;
use egui_kittest::{kittest::Queryable, Harness, SnapshotResults};
use modama_agent_gui::{
    app::AgentGuiApp,
    async_state::AsyncState,
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
        app.app_state.creds = Some(Credentials {
            username: "representative.user".to_string(),
            password: "representative-password".to_string(),
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
    assert!(loading
        .query_by(|node| {
            node.role() == egui::accesskit::Role::TextInput
                && node.label().as_deref() == Some("Search")
                && node.is_disabled()
        })
        .is_some());
    assert!(loading
        .query_by(|node| {
            node.role() == egui::accesskit::Role::ComboBox
                && node.label().as_deref() == Some("Sort by")
                && node.is_disabled()
        })
        .is_some());
    assert!(loading
        .query_by_role(egui::accesskit::Role::ProgressIndicator)
        .is_some());
    let progress = loading
        .get_by_role(egui::accesskit::Role::ProgressIndicator)
        .rect();
    let loading_label = loading.get_by_label("Fetching projects…").rect();
    let loading_group = progress.union(loading_label);
    let sort = loading
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
        .rect();
    let action = loading.get_by_label("Select Project").rect();
    assert!((loading_group.center().x - VIEWPORT.x / 2.0).abs() <= 1.0);
    assert!((loading_group.center().y - (sort.bottom() + action.top()) / 2.0).abs() <= 3.0);
    loading.snapshot("select_project_loading");
    release.wait();
    results.extend_harness(&mut loading);

    let mut no_access_page = PageSelectProject::new();
    no_access_page.projects_task = AsyncState::Ready {
        payload: Vec::new(),
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
    let hovered_row = hovered.get_by_label_contains("Holo Stacks, owner").rect();
    hovered.hover_at(egui::pos2(hovered_row.left() + 2.0, hovered_row.center().y));
    hovered.run_steps(2);
    hovered.snapshot("select_project_hovered");
    results.extend_harness(&mut hovered);

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
        .get_by_role_and_label(egui::accesskit::Role::Button, "Name A–Z")
        .hover();
    sort_hovered.run_steps(2);
    sort_hovered.snapshot("select_project_sort_item_hovered");
    results.extend_harness(&mut sort_hovered);

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
fn project_sort_dropdown_options_are_accessible_and_selectable() {
    let mut page = PageSelectProject::new();
    page.available_projects = representative_projects();
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    harness
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort by")
        .click();
    harness.run_steps(2);

    assert!(harness
        .query_by(|node| {
            node.role() == egui::accesskit::Role::Button
                && node.label().as_deref() == Some("Last modified")
                && node.toggled() == Some(egui::accesskit::Toggled::True)
        })
        .is_some());
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Name A–Z")
        .click();
    harness.run_steps(2);

    let Page::SelectProject(page) = &harness.state().page else {
        panic!("project selection page unexpectedly changed");
    };
    assert_eq!(page.sort, ProjectSort::Name);
    assert!(harness
        .query_by_role_and_label(egui::accesskit::Role::Button, "Name A–Z")
        .is_none());
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
        payload: Vec::new(),
    };
    let mut harness = selection_harness(Page::SelectProject(page));
    harness.run_steps(3);

    assert!(harness.query_by_label("No projects available").is_some());
    assert!(harness
        .query_by_label("You don’t have access to any projects.")
        .is_some());
    assert!(harness
        .query_by_role(egui::accesskit::Role::ProgressIndicator)
        .is_none());

    harness.run_steps(3);
    let Page::SelectProject(page) = &harness.state().page else {
        panic!("project selection page unexpectedly changed");
    };
    assert!(matches!(page.projects_task, AsyncState::Idle));
    assert!(!page.is_busy());
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
    assert!(harness.query_by_label("Select Project").is_some());
    assert!(harness
        .query_by_role(egui::accesskit::Role::RadioButton)
        .is_none());
    assert!(harness
        .query_by(|node| {
            node.label()
                .is_some_and(|label| label.starts_with("Holo Stacks, owner"))
                && node.toggled() == Some(egui::accesskit::Toggled::True)
        })
        .is_some());
    assert!(harness
        .query_all_by_label("1 additional member")
        .next()
        .is_some());
    assert!(harness
        .query_all_by_label("2 additional members")
        .next()
        .is_some());
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
        let guide = harness.get_by_label("go to Overview").rect().left();
        let heading = harness.get_by_label("Select a project").rect();
        let card_outer_left = styles::content_card_outer_inset_from_window();
        let card_border = styles::components::standard_border::width::EGUI_POINTS;
        assert!((heading.left() - guide).abs() <= 1.0);
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
