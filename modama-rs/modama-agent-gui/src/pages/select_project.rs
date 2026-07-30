use std::{cmp::Ordering, time::Duration};

use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, TextEdit, WidgetInfo};
use time::OffsetDateTime;

use crate::{
    actions::Action,
    async_state::AsyncState,
    buttons::primary_button,
    components::{avatar, field_label, labeled_text_input_with_insets, page_heading},
    styles,
};

const PROJECT_ROW_HEIGHT: f32 = 64.0;
const MEMBER_AVATAR_SIZE: f32 = 24.0;
const MAX_VISIBLE_AVATARS: usize = 5;
const MEMBER_AVATAR_OVERLAP: f32 = 5.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummary {
    pub name: String,
    pub owner: String,
    pub members: Vec<String>,
    pub modified_at: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectSort {
    #[default]
    Modified,
    Name,
}

impl ProjectSort {
    fn label(self) -> &'static str {
        match self {
            Self::Modified => "Last modified",
            Self::Name => "Name A–Z",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FetchProjectsRequest;

impl FetchProjectsRequest {
    pub fn execute(&self) -> Vec<ProjectSummary> {
        std::thread::sleep(Duration::from_millis(500));
        representative_projects()
    }
}

pub struct PageSelectProject {
    pub selected_project: Option<String>,
    pub projects_task: AsyncState<Vec<ProjectSummary>>,
    pub available_projects: Vec<ProjectSummary>,
    pub search_query: String,
    pub sort: ProjectSort,
    initial_fetch_complete: bool,
}

impl Default for PageSelectProject {
    fn default() -> Self {
        Self::new()
    }
}

impl PageSelectProject {
    pub fn new() -> Self {
        Self {
            selected_project: None,
            projects_task: AsyncState::Idle,
            available_projects: Vec::new(),
            search_query: String::new(),
            sort: ProjectSort::Modified,
            initial_fetch_complete: false,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        if !self.initial_fetch_complete
            && matches!(self.projects_task, AsyncState::Idle)
            && self.available_projects.is_empty()
        {
            let ctx = ui.ctx().clone();
            self.projects_task
                .spawn(move || FetchProjectsRequest.execute(), ctx);
        }

        page_heading(ui, "Select a project");
        ui.add_space(styles::content_card_top_margin_adjustment());

        let is_loading = self.projects_task.is_pending();
        ui.add_enabled_ui(!is_loading, |ui| self.show_toolbar(ui));
        ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);

        let visible_projects =
            filter_and_sort_projects(&self.available_projects, &self.search_query, self.sort);
        let list_state = if is_loading {
            ProjectListState::Loading
        } else if self.available_projects.is_empty() {
            ProjectListState::NoAccess
        } else if visible_projects.is_empty() {
            ProjectListState::NoSearchMatches
        } else {
            ProjectListState::Projects
        };
        let selected_is_visible = self.selected_project.as_ref().is_some_and(|selected| {
            visible_projects
                .iter()
                .any(|project| project.name == *selected)
        });
        let mut action = Action::None;
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
            ui.horizontal(|ui| {
                ui.add_space(content_guide_widget_offset(
                    styles::spacing::button_padding_x::EGUI_POINTS,
                ));
                if ui
                    .add_enabled(selected_is_visible, primary_button("Select Project", ui))
                    .clicked()
                {
                    if let Some(project) = &self.selected_project {
                        action = Action::SelectProject {
                            project: project.clone(),
                        };
                    }
                }
            });
            ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);

            let list_outer_height = ui.available_height();
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), list_outer_height),
                Layout::top_down(Align::Min),
                |ui| self.show_project_list(ui, &visible_projects, list_outer_height, list_state),
            );
        });
        action
    }

    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_top(|ui| {
            let sort_width = 172.0;
            let search_width =
                (ui.available_width() - sort_width - styles::spacing::app_item_gap_x::EGUI_POINTS)
                    .max(180.0);

            let search = ui
                .allocate_ui(egui::vec2(search_width, 0.0), |ui| {
                    labeled_text_input_with_insets(
                        ui,
                        "Search",
                        TextEdit::singleline(&mut self.search_query).hint_text("Search projects…"),
                        styles::content_card_inset(),
                        styles::spacing::button_padding_x::EGUI_POINTS,
                    )
                })
                .inner;
            ui.allocate_ui(egui::vec2(sort_width, 0.0), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.spacing_mut().interact_size.y = search.rect.height();
                    let label = field_label(ui, "Sort by");
                    ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
                    let surface = styles::color::surface::VALUE.resolve_egui(ui.visuals());
                    let surface_variant =
                        styles::color::surface_variant::VALUE.resolve_egui(ui.visuals());
                    let outline = styles::color::outline::VALUE.resolve_egui(ui.visuals());
                    let primary_tint =
                        styles::color::primary_tint::VALUE.resolve_egui(ui.visuals());
                    ui.visuals_mut().widgets.inactive.bg_fill = surface;
                    ui.visuals_mut().widgets.inactive.weak_bg_fill = surface;
                    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(
                        styles::components::standard_border::width::EGUI_POINTS,
                        outline,
                    );
                    let widgets = &mut ui.visuals_mut().widgets;
                    for widget in [&mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
                        widget.bg_fill = surface_variant;
                        widget.weak_bg_fill = surface_variant;
                        widget.bg_stroke = Stroke::new(
                            styles::components::standard_border::width::EGUI_POINTS,
                            outline,
                        );
                    }
                    let response = egui::ComboBox::from_id_salt("project-sort")
                        .width(ui.available_width())
                        .selected_text(self.sort.label())
                        .popup_style(egui::style::StyleModifier::new(
                            move |style: &mut egui::Style| {
                                egui::containers::menu::menu_style(style);
                                style.spacing.menu_margin.left = 0;
                                style.spacing.menu_margin.right = 0;
                                style.spacing.item_spacing.y = 0.0;
                                style.spacing.button_padding = egui::vec2(
                                    styles::spacing::input_padding_x::EGUI_POINTS,
                                    styles::spacing::button_padding_y::EGUI_POINTS,
                                );
                                style.visuals.selection.bg_fill = primary_tint;
                                for widget in [
                                    &mut style.visuals.widgets.inactive,
                                    &mut style.visuals.widgets.hovered,
                                    &mut style.visuals.widgets.active,
                                    &mut style.visuals.widgets.open,
                                ] {
                                    widget.corner_radius = egui::CornerRadius::ZERO;
                                    widget.bg_stroke = Stroke::NONE;
                                }
                                style.visuals.widgets.inactive.bg_fill = surface;
                                style.visuals.widgets.inactive.weak_bg_fill = surface;
                                for widget in [
                                    &mut style.visuals.widgets.hovered,
                                    &mut style.visuals.widgets.active,
                                    &mut style.visuals.widgets.open,
                                ] {
                                    widget.bg_fill = surface_variant;
                                    widget.weak_bg_fill = surface_variant;
                                }
                            },
                        ))
                        .show_ui(ui, |ui| {
                            sort_option(ui, &mut self.sort, ProjectSort::Modified);
                            sort_option(ui, &mut self.sort, ProjectSort::Name);
                        })
                        .response
                        .labelled_by(label.id);
                    styles::paint_focus_ring(ui, &response);
                });
            });
        });
    }

    fn show_project_list(
        &mut self,
        ui: &mut egui::Ui,
        projects: &[ProjectSummary],
        outer_height: f32,
        state: ProjectListState,
    ) {
        let frame_vertical_padding = 4.0 * styles::spacing::button_padding_y::EGUI_POINTS;
        let frame_border = 2.0 * styles::components::standard_border::width::EGUI_POINTS;
        let list_height =
            (outer_height - frame_vertical_padding - frame_border).max(PROJECT_ROW_HEIGHT);
        let mut list_frame = styles::content_card_frame(ui.visuals());
        list_frame.inner_margin.left = 0;
        list_frame.inner_margin.right = 0;
        list_frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            match state {
                ProjectListState::Loading => {
                    let loading_text = "Fetching projects…";
                    let loading_text_width = ui
                        .painter()
                        .layout_no_wrap(
                            loading_text.to_owned(),
                            styles::typography::body::font_id(),
                            ui.visuals().text_color(),
                        )
                        .size()
                        .x;
                    let loading_width = ui.spacing().interact_size.y
                        + ui.spacing().item_spacing.x
                        + loading_text_width;
                    centered_list_state(
                        ui,
                        list_height,
                        loading_width,
                        24.0,
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new(loading_text)
                                    .font(styles::typography::body::font_id()),
                            );
                        },
                    );
                    return;
                }
                ProjectListState::NoAccess => {
                    centered_list_state(
                        ui,
                        list_height,
                        ui.available_width(),
                        62.0,
                        Layout::top_down(Align::Center),
                        |ui| {
                            ui.label(
                                RichText::new("No projects available")
                                    .font(styles::typography::section_heading::font_id()),
                            );
                            ui.label(
                                RichText::new("You don’t have access to any projects.")
                                    .font(styles::typography::body::font_id())
                                    .color(
                                        styles::color::on_surface_variant::VALUE
                                            .resolve_egui(ui.visuals()),
                                    ),
                            );
                        },
                    );
                    return;
                }
                ProjectListState::NoSearchMatches => {
                    centered_list_state(
                        ui,
                        list_height,
                        ui.available_width(),
                        62.0,
                        Layout::top_down(Align::Center),
                        |ui| {
                            ui.label(
                                RichText::new("No projects found")
                                    .font(styles::typography::section_heading::font_id()),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Nothing matches “{}”.",
                                    self.search_query.trim()
                                ))
                                .font(styles::typography::body::font_id())
                                .color(
                                    styles::color::on_surface_variant::VALUE
                                        .resolve_egui(ui.visuals()),
                                ),
                            );
                        },
                    );
                    return;
                }
                ProjectListState::Projects => {}
            }

            egui::ScrollArea::vertical()
                .id_salt("project-list")
                .auto_shrink([false, false])
                .max_height(list_height)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (index, project) in projects.iter().enumerate() {
                        let selected =
                            self.selected_project.as_deref() == Some(project.name.as_str());
                        if project_row(ui, project, selected, index + 1 < projects.len()).clicked()
                        {
                            self.selected_project = Some(project.name.clone());
                        }
                    }
                });
        });
    }

    pub fn poll(&mut self) -> Action {
        if let Some(projects) = self.projects_task.poll().take_ready() {
            self.available_projects = projects;
            self.initial_fetch_complete = true;
        }
        Action::None
    }

    pub fn is_busy(&self) -> bool {
        self.projects_task.is_pending()
            || (!self.initial_fetch_complete
                && matches!(self.projects_task, AsyncState::Idle)
                && self.available_projects.is_empty())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectListState {
    Loading,
    NoAccess,
    NoSearchMatches,
    Projects,
}

fn sort_option(
    ui: &mut egui::Ui,
    current_sort: &mut ProjectSort,
    option: ProjectSort,
) -> egui::Response {
    let selected = *current_sort == option;
    let mut response = ui.add(
        egui::Button::selectable(selected, option.label())
            .corner_radius(egui::CornerRadius::ZERO)
            .min_size(egui::vec2(ui.available_width(), 0.0)),
    );
    if response.clicked() && !selected {
        *current_sort = option;
        response.mark_changed();
    }
    response.widget_info(|| {
        WidgetInfo::selected(
            egui::WidgetType::Button,
            ui.is_enabled(),
            selected,
            option.label(),
        )
    });
    response
}

fn centered_list_state(
    ui: &mut egui::Ui,
    height: f32,
    content_width: f32,
    content_height: f32,
    layout: Layout,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let available = ui.available_rect_before_wrap();
    let (_, reserved) = ui.allocate_space(egui::vec2(available.width(), height));
    let content_rect = egui::Rect::from_min_size(
        egui::pos2(
            reserved.center().x - content_width / 2.0,
            reserved.top() + ((height - content_height) / 2.0).max(0.0),
        ),
        egui::vec2(content_width, content_height),
    );
    let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect).layout(layout));
    add_contents(&mut content_ui);
}

fn content_guide_widget_offset(horizontal_padding: f32) -> f32 {
    (styles::content_card_inset() - horizontal_padding).max(0.0)
}

fn additional_members_avatar(ui: &mut egui::Ui, count: usize) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(MEMBER_AVATAR_SIZE), Sense::hover());
    let visuals = ui.visuals();
    ui.painter().circle_filled(
        rect.center(),
        MEMBER_AVATAR_SIZE / 2.0,
        styles::color::surface_variant::VALUE.resolve_egui(visuals),
    );
    ui.painter().circle_stroke(
        rect.center(),
        MEMBER_AVATAR_SIZE / 2.0,
        Stroke::new(
            styles::components::standard_border::width::EGUI_POINTS,
            styles::color::outline::VALUE.resolve_egui(visuals),
        ),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("+{count}"),
        styles::typography::supporting_text::font_id(),
        styles::color::on_surface_variant::VALUE.resolve_egui(visuals),
    );
    let description = match count {
        1 => "1 additional member".to_owned(),
        count => format!("{count} additional members"),
    };
    response.widget_info(|| {
        WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &description)
    });
    response.on_hover_text(description)
}

fn project_row(
    ui: &mut egui::Ui,
    project: &ProjectSummary,
    selected: bool,
    show_separator: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), PROJECT_ROW_HEIGHT),
        Sense::click(),
    );
    let visuals = ui.visuals();
    let content_rect = rect.shrink2(egui::vec2(
        styles::spacing::button_padding_x::EGUI_POINTS,
        0.0,
    ));
    let fill = if selected {
        styles::color::primary_tint::VALUE.resolve_egui(visuals)
    } else if response.hovered() {
        styles::color::surface_variant::VALUE.resolve_egui(visuals)
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    if show_separator {
        ui.painter().hline(
            content_rect.x_range(),
            rect.bottom(),
            Stroke::new(
                styles::components::standard_border::width::EGUI_POINTS,
                styles::color::outline::VALUE.resolve_egui(visuals),
            ),
        );
    }

    let text_left = content_rect.left();
    ui.painter().text(
        egui::pos2(text_left, rect.top() + 12.0),
        egui::Align2::LEFT_TOP,
        &project.name,
        styles::typography::mono_medium::font_id(),
        styles::color::on_surface::VALUE.resolve_egui(visuals),
    );
    ui.painter().text(
        egui::pos2(text_left, rect.bottom() - 12.0),
        egui::Align2::LEFT_BOTTOM,
        format!(
            "Owner: {}  ·  {}",
            project.owner,
            member_count_label(project.members.len())
        ),
        styles::typography::supporting_text::font_id(),
        styles::color::on_surface_dim::VALUE.resolve_egui(visuals),
    );

    let modified = format_modified_date(project.modified_at);
    ui.painter().text(
        egui::pos2(content_rect.right() - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        modified,
        styles::typography::supporting_text::font_id(),
        styles::color::on_surface_variant::VALUE.resolve_egui(visuals),
    );

    let people = std::iter::once(project.owner.as_str())
        .chain(project.members.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let overflow_count = people.len().saturating_sub(MAX_VISIBLE_AVATARS);
    let avatars_rect = member_avatars_rect(content_rect, people.len());
    let mut avatars_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(avatars_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    avatars_ui.spacing_mut().item_spacing.x = -MEMBER_AVATAR_OVERLAP;
    if overflow_count > 0 {
        additional_members_avatar(&mut avatars_ui, overflow_count);
    }
    for username in people.iter().take(MAX_VISIBLE_AVATARS).rev() {
        avatar(&mut avatars_ui, username, MEMBER_AVATAR_SIZE);
    }

    response.widget_info(|| {
        WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!(
                "{}, owner {}, {}, modified {}",
                project.name,
                project.owner,
                member_count_label(project.members.len()),
                format_modified_date(project.modified_at)
            ),
        )
    });
    styles::paint_focus_ring(ui, &response);
    response
}

fn member_avatars_rect(row_rect: egui::Rect, people_count: usize) -> egui::Rect {
    let avatar_count = people_count.min(MAX_VISIBLE_AVATARS);
    let displayed_avatar_count = avatar_count + usize::from(people_count > MAX_VISIBLE_AVATARS);
    let avatars_width = displayed_avatar_count as f32
        * (MEMBER_AVATAR_SIZE - MEMBER_AVATAR_OVERLAP)
        + MEMBER_AVATAR_OVERLAP;
    let avatars_right = row_rect.right() - 126.0;
    egui::Rect::from_min_size(
        egui::pos2(
            avatars_right - avatars_width,
            row_rect.center().y - MEMBER_AVATAR_SIZE / 2.0,
        ),
        egui::vec2(avatars_width, MEMBER_AVATAR_SIZE),
    )
}

fn member_count_label(member_count: usize) -> String {
    match member_count {
        1 => "1 member".to_owned(),
        count => format!("{count} members"),
    }
}

fn format_modified_date(timestamp: i64) -> String {
    let Ok(date_time) = OffsetDateTime::from_unix_timestamp(timestamp) else {
        return "Unknown date".to_owned();
    };
    let date = date_time.date();
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

pub fn filter_and_sort_projects(
    projects: &[ProjectSummary],
    query: &str,
    sort: ProjectSort,
) -> Vec<ProjectSummary> {
    let normalized_query = query.trim().to_lowercase();
    let mut matches = if normalized_query.is_empty() {
        projects.to_vec()
    } else {
        let substring_matches = projects
            .iter()
            .filter(|project| project.name.to_lowercase().contains(&normalized_query))
            .cloned()
            .collect::<Vec<_>>();
        if !substring_matches.is_empty() {
            substring_matches
        } else if normalized_query.chars().count() >= 3 {
            let mut scored = projects
                .iter()
                .filter_map(|project| {
                    let score = strsim::jaro_winkler(
                        &project.name.to_lowercase(),
                        normalized_query.as_str(),
                    );
                    (score >= 0.72).then_some((score, project.clone()))
                })
                .collect::<Vec<_>>();
            scored.sort_by(|(left_score, left_project), (right_score, right_project)| {
                right_score
                    .partial_cmp(left_score)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| compare_projects(left_project, right_project, sort))
            });
            return scored.into_iter().map(|(_, project)| project).collect();
        } else {
            Vec::new()
        }
    };
    matches.sort_by(|left, right| compare_projects(left, right, sort));
    matches
}

fn compare_projects(left: &ProjectSummary, right: &ProjectSummary, sort: ProjectSort) -> Ordering {
    match sort {
        ProjectSort::Modified => right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| compare_names(left, right)),
        ProjectSort::Name => compare_names(left, right),
    }
}

fn compare_names(left: &ProjectSummary, right: &ProjectSummary) -> Ordering {
    left.name
        .to_lowercase()
        .cmp(&right.name.to_lowercase())
        .then_with(|| left.name.cmp(&right.name))
}

pub fn representative_projects() -> Vec<ProjectSummary> {
    const NAMES: [(&str, &str, i64); 24] = [
        ("Adaptive Optics", "mira", 1_775_779_200),
        ("Aerosol Chamber", "nora", 1_771_344_000),
        ("Aurora Spectroscopy", "alex", 1_774_137_600),
        ("Biofilm Dynamics", "sam", 1_768_838_400),
        ("Cryogenic Detector", "weber", 1_772_899_200),
        ("Deep Field Calibration", "mira", 1_766_246_400),
        ("Dust Polarization", "shanker", 1_773_763_200),
        ("Electron Microscopy", "alex", 1_769_529_600),
        ("Exoplanet Atmospheres", "nora", 1_774_742_400),
        ("Gamma Ray Timing", "sam", 1_767_024_000),
        ("Holo Stacks", "weber", 1_775_865_600),
        ("Holo Stacks Archive", "weber", 1_763_654_400),
        ("Hydrogen Mapping", "mira", 1_770_739_200),
        ("Interferometer Tests", "shanker", 1_772_121_600),
        ("Laser Alignment", "alex", 1_765_382_400),
        ("Lunar Regolith", "nora", 1_771_948_800),
        ("Magnetic Survey", "sam", 1_769_011_200),
        ("Neutrino Runs", "weber", 1_775_174_400),
        ("Optical Bench 7", "mira", 1_768_406_400),
        ("Plasma Diagnostics", "shanker", 1_773_244_800),
        ("Quantum Sensor Array", "alex", 1_770_220_800),
        ("Radio Transients", "nora", 1_774_310_400),
        ("Solar Flare Imaging", "sam", 1_766_937_600),
        ("Wideband Receiver", "weber", 1_771_084_800),
    ];
    const USERS: [&str; 8] = [
        "alex", "mira", "nora", "sam", "shanker", "weber", "yuki", "zoe",
    ];

    NAMES
        .iter()
        .enumerate()
        .map(|(index, (name, owner, modified_at))| ProjectSummary {
            name: (*name).to_owned(),
            owner: (*owner).to_owned(),
            members: (0..(index % 7))
                .map(|offset| USERS[(index + offset + 1) % USERS.len()].to_owned())
                .filter(|username| username != owner)
                .collect(),
            modified_at: *modified_at,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str, modified_at: i64) -> ProjectSummary {
        ProjectSummary {
            name: name.to_owned(),
            owner: "owner".to_owned(),
            members: vec!["member".to_owned()],
            modified_at,
        }
    }

    #[test]
    fn representative_data_has_more_than_twenty_projects() {
        assert!(representative_projects().len() > 20);
    }

    #[test]
    fn avatar_stacks_keep_their_right_edge_when_overflow_is_present() {
        let row = egui::Rect::from_min_size(egui::pos2(40.0, 20.0), egui::vec2(900.0, 64.0));
        let expected_right = row.right() - 126.0;

        for people_count in [1, 3, 6, 12] {
            assert_eq!(
                member_avatars_rect(row, people_count).right(),
                expected_right
            );
        }
    }

    #[test]
    fn sorts_by_name_or_most_recent() {
        let projects = vec![
            project("Zulu", 10),
            project("alpha", 20),
            project("Beta", 20),
        ];
        let by_name = filter_and_sort_projects(&projects, "", ProjectSort::Name);
        assert_eq!(
            by_name
                .iter()
                .map(|project| project.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "Beta", "Zulu"]
        );

        let by_modified = filter_and_sort_projects(&projects, "", ProjectSort::Modified);
        assert_eq!(
            by_modified
                .iter()
                .map(|project| project.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "Beta", "Zulu"]
        );
    }

    #[test]
    fn substring_matches_take_priority_over_fuzzy_matches() {
        let projects = vec![
            project("Holo Stacks", 1),
            project("Holography", 2),
            project("Halo Survey", 3),
        ];
        let matches = filter_and_sort_projects(&projects, "holo", ProjectSort::Name);
        assert_eq!(
            matches
                .iter()
                .map(|project| project.name.as_str())
                .collect::<Vec<_>>(),
            ["Holo Stacks", "Holography"]
        );
    }

    #[test]
    fn fuzzy_search_is_used_only_when_substring_search_is_empty() {
        let projects = vec![project("Holo Stacks", 1), project("Radio Survey", 2)];
        let matches = filter_and_sort_projects(&projects, "hollo staks", ProjectSort::Name);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "Holo Stacks");
        assert!(filter_and_sort_projects(&projects, "zz", ProjectSort::Name).is_empty());
    }
}
