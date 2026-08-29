use crate::components::{avatar::avatar, dropdown};
use std::{cmp::Ordering, fmt::Display, sync::Arc};

use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, TextEdit, WidgetInfo};
use egui_extras::{Size, StripBuilder};
use egui_taffy::{
    TuiBuilderLogic, TuiContainerResponse,
    taffy::{
        self,
        prelude::{AlignItems, FlexDirection, FlexWrap, Position, Style, auto, length, percent},
    },
    tui,
};
use time::OffsetDateTime;

use crate::{
    actions::{Action, RetryAction, TaskFailure},
    async_state::AsyncState,
    components::{
        buttons::primary_button,
        flow_navigation::{FlowStage, flow_navigation},
        forms::labeled_text_input_with_insets,
        list_view::{RovingListState, show_roving_list},
        page_heading,
    },
    service::{AgentService, ServiceError, StubAgentService},
    state::Credentials,
    styles,
};

pub use crate::service::ProjectSummary;

const MAX_VISIBLE_AVATARS: usize = 5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectSort {
    #[default]
    Modified,
    Name,
}

impl Display for ProjectSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectSort::Modified => write!(f, "Last modified"),
            ProjectSort::Name => write!(f, "Name A-Z"),
        }
    }
}

pub struct PageSelectProject {
    pub selected_project: Option<String>,
    pub projects_task: AsyncState<Result<Vec<ProjectSummary>, ServiceError>>,
    pub available_projects: Vec<ProjectSummary>,
    pub search_query: String,
    pub sort: ProjectSort,
    focused_project: RovingListState,
    focus_search_on_entry: bool,
    service: Arc<dyn AgentService>,
    credentials: Credentials,
}

impl Default for PageSelectProject {
    fn default() -> Self {
        Self::new()
    }
}

impl PageSelectProject {
    pub fn new() -> Self {
        Self::with_service(
            StubAgentService::shared(),
            Credentials {
                username: "representative.user".to_owned(),
                password: "representative-password".to_owned(),
            },
        )
    }

    pub fn with_service(service: Arc<dyn AgentService>, credentials: Credentials) -> Self {
        Self {
            selected_project: None,
            projects_task: AsyncState::Idle,
            available_projects: Vec::new(),
            search_query: String::new(),
            sort: ProjectSort::Modified,
            focused_project: RovingListState::default(),
            focus_search_on_entry: true,
            service,
            credentials,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        self.start_initial_fetch(ui);

        if let Some(stage) = flow_navigation(ui, FlowStage::Project, !self.is_busy()) {
            return match stage {
                FlowStage::Overview => Action::GoToOverview,
                FlowStage::Project | FlowStage::Session => Action::None,
            };
        }
        ui.add_space(styles::spacing::header_title_gap::EGUI_POINTS);
        page_heading(ui, "Select a project");
        ui.add_space(styles::content_card_top_margin_adjustment());
        self.show_page_content(ui)
    }

    fn start_initial_fetch(&mut self, ui: &egui::Ui) {
        if self.projects_task.is_idle() && self.available_projects.is_empty() {
            let ctx = ui.ctx().clone();
            let service = Arc::clone(&self.service);
            let credentials = self.credentials.clone();
            self.projects_task
                .spawn(move || service.list_projects(&credentials), ctx);
        }
    }

    fn show_page_content(&mut self, ui: &mut egui::Ui) -> Action {
        let is_loading = self.projects_task.is_pending();
        let view_state = self.project_view_state(is_loading);
        ui.vertical(|ui| {
            ui.add_enabled_ui(!is_loading, |ui| self.show_toolbar(ui));
            ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
            self.show_results_and_action(
                ui,
                &view_state.visible_projects,
                view_state.list_state,
                view_state.selected_is_visible,
            )
        })
        .inner
    }

    fn project_view_state(&self, is_loading: bool) -> ProjectViewState {
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

        ProjectViewState {
            visible_projects,
            list_state,
            selected_is_visible,
        }
    }

    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        tui(ui, ui.id().with("project-toolbar"))
            .reserve_available_width()
            .style(toolbar_layout_style())
            .show(|tui| {
                tui.id("project-search")
                    .style(Style {
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        flex_basis: length(0.0_f32),
                        min_size: taffy::Size {
                            width: length(0.0_f32),
                            height: auto(),
                        },
                        ..Default::default()
                    })
                    .ui(|ui| {
                        let response = labeled_text_input_with_insets(
                            ui,
                            "Search",
                            TextEdit::singleline(&mut self.search_query)
                                .hint_text("Search projects…"),
                            styles::content_card_inset(),
                            styles::spacing::button_padding_x::EGUI_POINTS,
                        );
                        if self.focus_search_on_entry && response.enabled() {
                            response.request_focus();
                            self.focus_search_on_entry = false;
                        }
                    });
                tui.id("project-sort")
                    .style(Style {
                        flex_shrink: 0.0,
                        size: taffy::Size {
                            width: length(
                                styles::components::project_sort_control::width::EGUI_POINTS,
                            ),
                            height: auto(),
                        },
                        ..Default::default()
                    })
                    .ui(|ui| {
                        dropdown::dropdown(
                            ui,
                            [ProjectSort::Modified, ProjectSort::Name].iter(),
                            &mut self.sort,
                            "Sort by",
                            "sort-projects",
                        );
                    });
            });
    }

    fn show_results_and_action(
        &mut self,
        ui: &mut egui::Ui,
        projects: &[ProjectSummary],
        state: ProjectListState,
        selected_is_visible: bool,
    ) -> Action {
        let mut available_rect = ui.available_rect_before_wrap();
        // The former bottom-up layout included the surrounding vertical item gaps in its extent.
        // Keep those token-derived bounds while letting Taffy control widget creation order.
        available_rect.max.y += 2.0 * styles::spacing::app_item_gap_y::EGUI_POINTS;
        tui(ui, ui.id().with("project-results-and-action"))
            .with_allocated_rect(available_rect)
            .style(results_layout_style())
            .show(|tui| {
                let list_action = tui
                    .id("project-results")
                    .style(project_list_layout_style())
                    .ui_manual(|ui, container| {
                        let inner = self.show_project_list(
                            ui,
                            projects,
                            container.full_container().height()
                                - styles::spacing::form_field_gap::EGUI_POINTS
                                + styles::components::standard_border::width::EGUI_POINTS,
                            state,
                        );
                        TuiContainerResponse {
                            inner,
                            min_size: ui.min_size(),
                            intrinsic_size: None,
                            max_size: ui.min_size(),
                            infinite: egui::Vec2b::TRUE,
                        }
                    });
                let button_action = tui
                    .id("project-action")
                    .style(project_action_layout_style())
                    .ui(|ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(content_guide_widget_offset(
                                styles::spacing::button_padding_x::EGUI_POINTS,
                            ));
                            let clicked = ui
                                .add_enabled(
                                    selected_is_visible,
                                    primary_button("Select Project", ui),
                                )
                                .clicked();
                            if clicked && let Some(project) = &self.selected_project {
                                return Some(Action::SelectProject {
                                    project: project.clone(),
                                });
                            }
                            None
                        })
                        .inner
                    });
                button_action.or(list_action).unwrap_or(Action::None)
            })
    }

    fn show_project_list(
        &mut self,
        ui: &mut egui::Ui,
        projects: &[ProjectSummary],
        outer_height: f32,
        state: ProjectListState,
    ) -> Option<Action> {
        let frame_vertical_padding = 4.0 * styles::spacing::button_padding_y::EGUI_POINTS;
        let frame_border = 2.0 * styles::components::standard_border::width::EGUI_POINTS;
        let list_height = (outer_height - frame_vertical_padding - frame_border)
            .max(styles::components::project_row::height::EGUI_POINTS);
        let mut list_frame = styles::content_card_frame(ui.visuals());
        list_frame.inner_margin.left = 0;
        list_frame.inner_margin.right = 0;
        list_frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match state {
                    ProjectListState::Loading => {
                        show_loading_state(ui, list_height);
                        return None;
                    }
                    ProjectListState::NoAccess => {
                        show_empty_state(
                            ui,
                            list_height,
                            "No projects available",
                            "You don’t have access to any projects.".to_owned(),
                        );
                        return None;
                    }
                    ProjectListState::NoSearchMatches => {
                        show_empty_state(
                            ui,
                            list_height,
                            "No projects found",
                            format!("Nothing matches “{}”.", self.search_query.trim()),
                        );
                        return None;
                    }
                    ProjectListState::Projects => {}
                }

                styles::scrollbar_scope(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("project-list")
                        .auto_shrink([false, false])
                        .max_height(list_height)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let selected_index =
                                self.selected_project.as_ref().and_then(|selected| {
                                    projects
                                        .iter()
                                        .position(|project| project.name == *selected)
                                });
                            let list_output = show_roving_list(
                                ui,
                                ui.id().with("project-rows"),
                                projects,
                                selected_index,
                                &mut self.focused_project,
                                false,
                                |ui, index, project, selected, focusable| {
                                    project_row(ui, project, selected, index > 0, focusable)
                                },
                            );
                            if let Some(index) = list_output.selection_changed {
                                self.selected_project = Some(projects[index].name.clone());
                            }
                            if let Some(index) = list_output.activated {
                                return Some(Action::SelectProject {
                                    project: projects[index].name.clone(),
                                });
                            }
                            None
                        })
                        .inner
                })
                .inner
            })
            .inner
    }

    pub fn poll(&mut self) -> Action {
        match self.projects_task.poll() {
            Ok(task) => {
                if let Some(result) = task.take_ready() {
                    match result {
                        Ok(projects) => self.available_projects = projects,
                        Err(error) => {
                            log::error!("project-fetch failed: {error}");
                            return Action::TaskFailed(TaskFailure::ReturnToOverview {
                                message: "Could not load projects.".to_owned(),
                                retry: RetryAction::FetchProjects,
                            });
                        }
                    }
                }
            }
            Err(error) => {
                log::error!("project-fetch task failed: {error}");
                return Action::TaskFailed(TaskFailure::ReturnToOverview {
                    message: "Could not load projects.".to_owned(),
                    retry: RetryAction::FetchProjects,
                });
            }
        }
        Action::None
    }

    pub fn is_busy(&self) -> bool {
        self.projects_task.is_pending()
            || (self.projects_task.is_idle() && self.available_projects.is_empty())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectListState {
    Loading,
    NoAccess,
    NoSearchMatches,
    Projects,
}

struct ProjectViewState {
    visible_projects: Vec<ProjectSummary>,
    list_state: ProjectListState,
    selected_is_visible: bool,
}

fn toolbar_layout_style() -> Style {
    Style {
        display: taffy::Display::Flex,
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::NoWrap,
        align_items: Some(AlignItems::Start),
        size: taffy::Size {
            width: percent(1.0_f32),
            height: auto(),
        },
        gap: taffy::Size {
            width: length(styles::spacing::app_item_gap_x::EGUI_POINTS),
            height: length(0.0_f32),
        },
        ..Default::default()
    }
}

fn results_layout_style() -> Style {
    Style {
        display: taffy::Display::Flex,
        flex_direction: FlexDirection::Column,
        flex_wrap: FlexWrap::NoWrap,
        align_items: Some(AlignItems::Stretch),
        size: taffy::Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        gap: taffy::Size {
            width: length(0.0_f32),
            height: length(styles::spacing::form_field_gap::EGUI_POINTS),
        },
        padding: taffy::Rect {
            left: length(0.0_f32),
            right: length(0.0_f32),
            top: length(0.0_f32),
            bottom: length(styles::spacing::form_field_gap::EGUI_POINTS),
        },
        ..Default::default()
    }
}

fn project_list_layout_style() -> Style {
    let frame_vertical_padding = 4.0 * styles::spacing::button_padding_y::EGUI_POINTS;
    let frame_border = 2.0 * styles::components::standard_border::width::EGUI_POINTS;
    Style {
        flex_grow: 1.0,
        flex_shrink: 1.0,
        flex_basis: length(0.0_f32),
        min_size: taffy::Size {
            width: auto(),
            height: length(
                styles::components::project_row::height::EGUI_POINTS
                    + frame_vertical_padding
                    + frame_border,
            ),
        },
        ..Default::default()
    }
}

fn project_action_layout_style() -> Style {
    Style {
        // Match the button's legacy bottom-up baseline within the token-sized action row.
        position: Position::Relative,
        inset: taffy::Rect {
            left: auto(),
            right: auto(),
            top: length(-4.0 * styles::components::standard_border::width::EGUI_POINTS),
            bottom: auto(),
        },
        flex_shrink: 0.0,
        size: taffy::Size {
            width: auto(),
            height: length(
                styles::typography::button::FONT_SIZE
                    + 2.0 * styles::spacing::button_padding_y::EGUI_POINTS,
            ),
        },
        ..Default::default()
    }
}

fn show_loading_state(ui: &mut egui::Ui, list_height: f32) {
    let loading_text = "Fetching projects…";
    let loading_text_size = ui
        .painter()
        .layout_no_wrap(
            loading_text.to_owned(),
            styles::typography::body::font_id(),
            ui.visuals().text_color(),
        )
        .size();
    let spinner_size = ui.spacing().interact_size.y;
    centered_list_state(
        ui,
        list_height,
        egui::vec2(
            spinner_size + ui.spacing().item_spacing.x + loading_text_size.x,
            spinner_size.max(loading_text_size.y),
        ),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spinner();
            ui.label(RichText::new(loading_text).font(styles::typography::body::font_id()));
        },
    );
}

fn show_empty_state(ui: &mut egui::Ui, list_height: f32, title: &str, description: String) {
    let title_size = ui
        .painter()
        .layout_no_wrap(
            title.to_owned(),
            styles::typography::section_heading::font_id(),
            ui.visuals().text_color(),
        )
        .size();
    let description_size = ui
        .painter()
        .layout_no_wrap(
            description.clone(),
            styles::typography::body::font_id(),
            styles::color::on_surface_variant::VALUE.resolve(ui.visuals()),
        )
        .size();
    centered_list_state(
        ui,
        list_height,
        egui::vec2(
            title_size.x.max(description_size.x),
            title_size.y + ui.spacing().item_spacing.y + description_size.y,
        ),
        Layout::top_down(Align::Center),
        |ui| {
            ui.label(RichText::new(title).font(styles::typography::section_heading::font_id()));
            ui.label(
                RichText::new(description)
                    .font(styles::typography::body::font_id())
                    .color(styles::color::on_surface_variant::VALUE.resolve(ui.visuals())),
            );
        },
    );
}

fn centered_list_state(
    ui: &mut egui::Ui,
    height: f32,
    content_size: egui::Vec2,
    layout: Layout,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let available = ui.available_rect_before_wrap();
    let (_, reserved) = ui.allocate_space(egui::vec2(available.width(), height));
    let content_rect = egui::Rect::from_center_size(reserved.center(), content_size);
    let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect).layout(layout));
    add_contents(&mut content_ui);
}

fn content_guide_widget_offset(horizontal_padding: f32) -> f32 {
    (styles::content_card_inset() - horizontal_padding).max(0.0)
}

fn additional_members_avatar(ui: &mut egui::Ui, count: usize) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::Vec2::splat(styles::components::project_member_avatar::size::EGUI_POINTS),
        Sense::hover(),
    );
    let visuals = ui.visuals();
    ui.painter().circle_filled(
        rect.center(),
        styles::components::project_member_avatar::size::EGUI_POINTS / 2.0,
        styles::color::surface_variant::VALUE.resolve(visuals),
    );
    ui.painter().circle_stroke(
        rect.center(),
        styles::components::project_member_avatar::size::EGUI_POINTS / 2.0,
        Stroke::new(
            styles::components::standard_border::width::EGUI_POINTS,
            styles::color::outline::VALUE.resolve(visuals),
        ),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("+{count}"),
        styles::typography::supporting_text::font_id(),
        styles::color::on_surface_variant::VALUE.resolve(visuals),
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
    show_top_separator: bool,
    focusable: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(
            ui.available_width(),
            styles::components::project_row::height::EGUI_POINTS,
        ),
        if focusable {
            Sense::click()
        } else {
            Sense::CLICK
        },
    );
    let visuals = ui.visuals();
    let content_rect = rect.shrink2(egui::vec2(
        styles::spacing::button_padding_x::EGUI_POINTS,
        0.0,
    ));
    let fill = if selected {
        styles::color::primary_tint::VALUE.resolve(visuals)
    } else if response.hovered() {
        styles::color::surface_variant::VALUE.resolve(visuals)
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    if show_top_separator {
        ui.painter().hline(
            content_rect.x_range(),
            rect.top(),
            Stroke::new(
                styles::components::standard_border::width::EGUI_POINTS,
                styles::color::outline::VALUE.resolve(visuals),
            ),
        );
    }
    let name_color = styles::color::on_surface::VALUE.resolve(visuals);
    let details_color = styles::color::on_surface_dim::VALUE.resolve(visuals);
    let modified_color = styles::color::on_surface_variant::VALUE.resolve(visuals);
    ui.painter().text(
        egui::pos2(
            content_rect.right() - styles::spacing::app_item_gap_x::EGUI_POINTS,
            rect.center().y,
        ),
        egui::Align2::RIGHT_CENTER,
        format_modified_date(project.modified_at),
        styles::typography::supporting_text::font_id(),
        modified_color,
    );

    let people = std::iter::once(project.owner.as_str())
        .chain(project.members.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let overflow_count = people.len().saturating_sub(MAX_VISIBLE_AVATARS);
    let avatars_width = member_avatars_width(people.len());
    let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    content_ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    StripBuilder::new(&mut content_ui)
        .size(Size::remainder())
        .size(Size::exact(avatars_width))
        .size(Size::exact(
            styles::components::project_modified_column::width::EGUI_POINTS,
        ))
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                StripBuilder::new(ui)
                    .size(Size::exact(
                        styles::components::project_row_padding_y::padding::EGUI_POINTS,
                    ))
                    .size(Size::remainder())
                    .size(Size::remainder())
                    .size(Size::exact(
                        styles::components::project_row_padding_y::padding::EGUI_POINTS,
                    ))
                    .vertical(|mut strip| {
                        strip.empty();
                        strip.cell(|ui| {
                            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&project.name)
                                            .font(styles::typography::mono_medium::font_id())
                                            .color(name_color),
                                    )
                                    .selectable(false)
                                    .sense(Sense::empty()),
                                );
                            });
                        });
                        strip.cell(|ui| {
                            ui.with_layout(Layout::left_to_right(Align::Max), |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!(
                                            "Owner: {}  ·  {}",
                                            project.owner,
                                            member_count_label(project.members.len())
                                        ))
                                        .font(styles::typography::supporting_text::font_id())
                                        .color(details_color),
                                    )
                                    .selectable(false)
                                    .sense(Sense::empty()),
                                );
                            });
                        });
                        strip.empty();
                    });
            });
            strip.cell(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x =
                        -styles::components::project_member_avatar_overlap::size::EGUI_POINTS;
                    if overflow_count > 0 {
                        additional_members_avatar(ui, overflow_count);
                    }
                    for username in people.iter().take(MAX_VISIBLE_AVATARS).rev() {
                        avatar(
                            ui,
                            username,
                            styles::components::project_member_avatar::size::EGUI_POINTS,
                        );
                    }
                });
            });
            strip.cell(|ui| {
                ui.allocate_space(ui.available_size());
            });
        });

    let response = styles::pointer_cursor(response);
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
    styles::paint_focus_rect(ui, &response);
    response
}

fn member_avatars_width(people_count: usize) -> f32 {
    let avatar_count = people_count.min(MAX_VISIBLE_AVATARS);
    let displayed_avatar_count = avatar_count + usize::from(people_count > MAX_VISIBLE_AVATARS);
    let avatar_size = styles::components::project_member_avatar::size::EGUI_POINTS;
    let overlap = styles::components::project_member_avatar_overlap::size::EGUI_POINTS;
    displayed_avatar_count as f32 * (avatar_size - overlap) + overlap
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
    fn avatar_stack_width_accounts_for_overlap_and_overflow() {
        assert_eq!(
            member_avatars_width(1),
            styles::components::project_member_avatar::size::EGUI_POINTS
        );
        assert_eq!(member_avatars_width(3), 62.0);
        assert_eq!(member_avatars_width(6), 119.0);
        assert_eq!(member_avatars_width(12), 119.0);
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
