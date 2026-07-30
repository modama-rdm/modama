use std::{cmp::Ordering, time::Duration};

use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, TextEdit, WidgetInfo};
use time::OffsetDateTime;

use crate::{
    actions::Action,
    async_state::AsyncState,
    buttons::primary_button,
    components::{avatar, labeled_text_input, page_heading},
    styles,
};

const PROJECT_ROW_HEIGHT: f32 = 64.0;
const MEMBER_AVATAR_SIZE: f32 = 24.0;
const MAX_VISIBLE_AVATARS: usize = 5;

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
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        if matches!(self.projects_task, AsyncState::Idle) && self.available_projects.is_empty() {
            let ctx = ui.ctx().clone();
            self.projects_task
                .spawn(move || FetchProjectsRequest.execute(), ctx);
        }

        page_heading(ui, "Select a project");
        ui.add_space(styles::content_card_top_margin_adjustment());

        if self.projects_task.is_pending() {
            loading_projects(ui);
            return Action::None;
        }

        self.show_toolbar(ui);
        ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);

        let visible_projects =
            filter_and_sort_projects(&self.available_projects, &self.search_query, self.sort);
        self.show_project_list(ui, &visible_projects);

        ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
        let selected_is_visible = self.selected_project.as_ref().is_some_and(|selected| {
            visible_projects
                .iter()
                .any(|project| project.name == *selected)
        });
        ui.horizontal(|ui| {
            ui.add_space(styles::content_card_inset());
            if ui
                .add_enabled(selected_is_visible, primary_button("Select Project", ui))
                .clicked()
            {
                if let Some(project) = &self.selected_project {
                    return Action::SelectProject {
                        project: project.clone(),
                    };
                }
            }
            Action::None
        })
        .inner
    }

    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(styles::content_card_inset());
            let sort_width = 172.0;
            let search_width =
                (ui.available_width() - sort_width - styles::spacing::app_item_gap_x::EGUI_POINTS)
                    .max(180.0);

            ui.allocate_ui(egui::vec2(search_width, 0.0), |ui| {
                labeled_text_input(
                    ui,
                    "Search",
                    TextEdit::singleline(&mut self.search_query).hint_text("Search projects…"),
                );
            });
            ui.allocate_ui(egui::vec2(sort_width, 0.0), |ui| {
                ui.vertical(|ui| {
                    let label = ui.label(
                        RichText::new("Sort by")
                            .font(styles::typography::label::font_id())
                            .color(
                                styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals()),
                            ),
                    );
                    ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
                    let surface = styles::color::surface::VALUE.resolve_egui(ui.visuals());
                    let surface_variant =
                        styles::color::surface_variant::VALUE.resolve_egui(ui.visuals());
                    let outline = styles::color::outline::VALUE.resolve_egui(ui.visuals());
                    ui.visuals_mut().widgets.inactive.bg_fill = surface;
                    ui.visuals_mut().widgets.inactive.weak_bg_fill = surface;
                    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(
                        styles::components::standard_border::width::EGUI_POINTS,
                        outline,
                    );
                    ui.visuals_mut().widgets.hovered.bg_fill = surface_variant;
                    ui.visuals_mut().widgets.hovered.weak_bg_fill = surface_variant;
                    let response = egui::ComboBox::from_id_salt("project-sort")
                        .width(ui.available_width())
                        .selected_text(self.sort.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.sort,
                                ProjectSort::Modified,
                                ProjectSort::Modified.label(),
                            );
                            ui.selectable_value(
                                &mut self.sort,
                                ProjectSort::Name,
                                ProjectSort::Name.label(),
                            );
                        })
                        .response
                        .labelled_by(label.id);
                    styles::paint_focus_ring(ui, &response);
                });
            });
        });
    }

    fn show_project_list(&mut self, ui: &mut egui::Ui, projects: &[ProjectSummary]) {
        let raw_list_height = (ui.available_height() - 70.0).clamp(192.0, 448.0);
        let list_height = (raw_list_height / PROJECT_ROW_HEIGHT).floor() * PROJECT_ROW_HEIGHT;
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if projects.is_empty() {
                ui.set_min_height(list_height);
                ui.vertical_centered(|ui| {
                    ui.add_space((list_height - 62.0) / 2.0);
                    ui.label(
                        RichText::new("No projects found")
                            .font(styles::typography::section_heading::font_id()),
                    );
                    ui.label(
                        RichText::new(format!("Nothing matches “{}”.", self.search_query.trim()))
                            .font(styles::typography::body::font_id())
                            .color(
                                styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals()),
                            ),
                    );
                });
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("project-list")
                .auto_shrink([false, false])
                .max_height(list_height)
                .show(ui, |ui| {
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
        }
        Action::None
    }

    pub fn is_busy(&self) -> bool {
        self.projects_task.is_pending()
            || (matches!(self.projects_task, AsyncState::Idle)
                && self.available_projects.is_empty())
    }
}

fn loading_projects(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(styles::content_card_inset());
        ui.spinner();
        ui.label(RichText::new("Fetching projects…").font(styles::typography::body::font_id()));
    });
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
    let fill = if selected {
        styles::color::primary_tint::VALUE.resolve_egui(visuals)
    } else if response.hovered() {
        styles::color::surface_variant::VALUE.resolve_egui(visuals)
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, styles::radius(styles::rounded::lg::EGUI_POINTS), fill);
    if show_separator {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(
                styles::components::standard_border::width::EGUI_POINTS,
                styles::color::outline::VALUE.resolve_egui(visuals),
            ),
        );
    }

    let indicator_center = egui::pos2(rect.left() + 10.0, rect.center().y);
    let indicator_color = if selected {
        styles::color::primary::VALUE.resolve_egui(visuals)
    } else {
        styles::color::outline_variant::VALUE.resolve_egui(visuals)
    };
    ui.painter()
        .circle_stroke(indicator_center, 6.0, Stroke::new(1.5, indicator_color));
    if selected {
        ui.painter()
            .circle_filled(indicator_center, 3.0, indicator_color);
    }

    let text_left = rect.left() + 28.0;
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
        egui::pos2(rect.right() - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        modified,
        styles::typography::supporting_text::font_id(),
        styles::color::on_surface_variant::VALUE.resolve_egui(visuals),
    );

    let people = std::iter::once(project.owner.as_str())
        .chain(project.members.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let avatar_count = people.len().min(MAX_VISIBLE_AVATARS);
    let avatars_right = rect.right() - 126.0;
    let overflow_width = if people.len() > MAX_VISIBLE_AVATARS {
        34.0
    } else {
        0.0
    };
    let avatars_width = avatar_count as f32 * (MEMBER_AVATAR_SIZE - 5.0) + 5.0 + overflow_width;
    let avatars_rect = egui::Rect::from_min_size(
        egui::pos2(
            avatars_right - avatars_width,
            rect.center().y - MEMBER_AVATAR_SIZE / 2.0,
        ),
        egui::vec2(avatars_width, MEMBER_AVATAR_SIZE),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(avatars_rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = -5.0;
            for username in people.iter().take(MAX_VISIBLE_AVATARS) {
                avatar(ui, username, MEMBER_AVATAR_SIZE);
            }
            if people.len() > MAX_VISIBLE_AVATARS {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(
                    RichText::new(format!("+{}", people.len() - MAX_VISIBLE_AVATARS))
                        .font(styles::typography::supporting_text::font_id())
                        .color(styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals())),
                );
            }
        },
    );

    response.widget_info(|| {
        WidgetInfo::selected(
            egui::WidgetType::RadioButton,
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
