use std::time::Duration;

use eframe::egui::{
    self, text::LayoutJob, Align, FontFamily, FontId, Label, Layout, Response, RichText, Sense,
    Stroke, TextEdit, WidgetInfo, WidgetType,
};
use egui_taffy::{
    taffy::{
        self,
        prelude::{auto, length, percent, AlignItems, FlexDirection, FlexWrap, Position, Style},
    },
    tui, TuiBuilderLogic, TuiContainerResponse,
};
use time::{Date, OffsetDateTime};

use crate::{
    actions::Action,
    async_state::AsyncState,
    components::{
        buttons::{loading_button, primary_button, secondary_button, ButtonVariant},
        display_error,
        forms::labeled_text_input,
        list_view::{show_roving_list, RovingListState},
        page_heading,
    },
    state::ActiveSession,
    styles,
};

const SESSION_ROW_HEIGHT: f32 = 48.0;
const ASYNC_REJECTION_SENTINEL: &str = "forbidden";
const ASYNC_REJECTION_MESSAGE: &str = "Session name was rejected by the server.";

#[derive(Debug, Clone)]
pub struct FetchSessionsRequest {
    pub project: String,
}

impl FetchSessionsRequest {
    pub fn execute(&self) -> Vec<String> {
        crate::simulated_work::sleep(Duration::from_millis(500));
        vec![
            format!("{}-holo-stacks", "2026-06-16"),
            format!("{}-holo-stacks", "2026-06-18"),
            format!("{}-holo-stacks", "2026-06-20"),
            format!("{}-holo-stacks", "2026-06-26"),
        ]
    }
}

#[derive(Debug, Clone)]
pub struct CreateIngestRequest {
    pub project: String,
    pub session: String,
}

impl CreateIngestRequest {
    pub fn execute(&self) -> ActiveSession {
        crate::simulated_work::sleep(Duration::from_millis(500));

        ActiveSession {
            project: self.project.clone(),
            session: self.session.clone(),
            mount_path: Some(format!("/todo/{}/{}/", self.project, self.session)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateSessionAndIngestRequest {
    pub project: String,
    pub session: String,
}

impl CreateSessionAndIngestRequest {
    pub fn execute(&self) -> Result<ActiveSession, String> {
        crate::simulated_work::sleep(Duration::from_millis(500));
        if self
            .session
            .to_lowercase()
            .contains(ASYNC_REJECTION_SENTINEL)
        {
            return Err(ASYNC_REJECTION_MESSAGE.to_owned());
        }

        Ok(CreateIngestRequest {
            project: self.project.clone(),
            session: self.session.clone(),
        }
        .execute())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SessionPageMode {
    #[default]
    SelectExisting,
    CreateNew,
}

pub struct PageSelectCreateSession {
    pub project: String,
    pub selected_session: Option<String>,
    pub new_session_input: String,
    pub new_session_error: Option<String>,
    pub sessions_task: AsyncState<Vec<String>>,
    pub create_ingest_task: AsyncState<ActiveSession>,
    pub create_session_task: AsyncState<Result<ActiveSession, String>>,
    pub available_sessions: Vec<String>,
    mode: SessionPageMode,
    focused_session: RovingListState,
    focus_on_entry: bool,
    focus_create_on_entry: bool,
    focus_session_input_on_entry: bool,
    initial_fetch_complete: bool,
}

impl PageSelectCreateSession {
    pub fn new(project: String) -> Self {
        Self {
            project,
            selected_session: None,
            new_session_input: session_name_prefix(today()),
            new_session_error: None,
            sessions_task: AsyncState::Idle,
            create_ingest_task: AsyncState::Idle,
            create_session_task: AsyncState::Idle,
            available_sessions: Vec::new(),
            mode: SessionPageMode::SelectExisting,
            focused_session: RovingListState::default(),
            focus_on_entry: true,
            focus_create_on_entry: false,
            focus_session_input_on_entry: false,
            initial_fetch_complete: false,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        self.handle_keyboard_shortcuts(ui);
        if !self.initial_fetch_complete
            && matches!(self.sessions_task, AsyncState::Idle)
            && self.available_sessions.is_empty()
        {
            let req = FetchSessionsRequest {
                project: self.project.clone(),
            };
            let ctx = ui.ctx().clone();
            self.sessions_task.spawn(move || req.execute(), ctx);
        }

        page_heading(
            ui,
            &format!("Select or create a session in project '{}'", self.project),
        );
        ui.add_space(styles::content_card_top_margin_adjustment());

        match self.mode {
            SessionPageMode::SelectExisting => self.show_session_selection(ui),
            SessionPageMode::CreateNew => self.show_create_form(ui),
        }

        Action::None
    }

    fn handle_keyboard_shortcuts(&mut self, ui: &mut egui::Ui) {
        let command_n =
            ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::N));
        if self.mode == SessionPageMode::SelectExisting
            && command_n
            && !self.sessions_task.is_pending()
            && !self.create_ingest_task.is_pending()
        {
            self.new_session_error = None;
            self.mode = SessionPageMode::CreateNew;
            self.focus_on_entry = false;
            self.focus_session_input_on_entry = true;
            return;
        }

        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && self.mode == SessionPageMode::CreateNew
            && !self.create_session_task.is_pending()
        {
            self.new_session_error = None;
            self.mode = SessionPageMode::SelectExisting;
            self.focus_session_input_on_entry = false;
            self.focus_on_entry = true;
            self.focus_create_on_entry = true;
        }
    }

    fn show_session_selection(&mut self, ui: &mut egui::Ui) {
        let is_loading = self.sessions_task.is_pending();
        let is_starting = self.create_ingest_task.is_pending();

        self.show_session_results_and_action(ui, is_loading, is_starting);
    }

    fn show_session_results_and_action(
        &mut self,
        ui: &mut egui::Ui,
        is_loading: bool,
        is_starting: bool,
    ) {
        let mut available_rect = ui.available_rect_before_wrap();
        // Preserve the extent previously provided by the bottom-up layout while
        // declaring the list before the action for keyboard navigation.
        available_rect.max.y += 2.0 * styles::spacing::app_item_gap_y::EGUI_POINTS;
        tui(ui, ui.id().with("session-results-and-action"))
            .with_allocated_rect(available_rect)
            .style(session_results_layout_style())
            .show(|tui| {
                tui.id("session-results")
                    .style(session_list_layout_style())
                    .ui_manual(|ui, container| {
                        self.show_session_list(
                            ui,
                            container.full_container().height()
                                - styles::spacing::form_field_gap::EGUI_POINTS
                                + styles::components::standard_border::width::EGUI_POINTS,
                            is_loading,
                            is_starting,
                        );
                        TuiContainerResponse {
                            inner: (),
                            min_size: ui.min_size(),
                            intrinsic_size: None,
                            max_size: ui.min_size(),
                            infinite: egui::Vec2b::TRUE,
                        }
                    });
                tui.id("session-action")
                    .style(session_action_layout_style())
                    .ui(|ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(content_guide_widget_offset(
                                styles::spacing::button_padding_x::EGUI_POINTS,
                            ));
                            let response = if is_starting {
                                loading_button(
                                    ui,
                                    "Starting…",
                                    egui::Vec2::ZERO,
                                    ButtonVariant::Primary,
                                )
                            } else {
                                ui.add_enabled(
                                    self.selected_session.is_some() && !is_loading,
                                    primary_button("Select Session", ui),
                                )
                            };
                            if response.clicked() {
                                self.start_existing_session(ui.ctx().clone());
                            }
                        });
                    });
            });
    }

    fn show_session_list(
        &mut self,
        ui: &mut egui::Ui,
        outer_height: f32,
        is_loading: bool,
        is_starting: bool,
    ) {
        let frame_vertical_padding = 4.0 * styles::spacing::button_padding_y::EGUI_POINTS;
        let frame_border = 2.0 * styles::components::standard_border::width::EGUI_POINTS;
        let list_height =
            (outer_height - frame_vertical_padding - frame_border).max(SESSION_ROW_HEIGHT);
        let mut list_frame = styles::content_card_frame(ui.visuals());
        list_frame.inner_margin.left = 0;
        list_frame.inner_margin.right = 0;
        list_frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            if is_loading {
                centered_loading(ui, list_height);
                return;
            }

            if is_starting {
                ui.disable();
            }
            ui.style_mut().spacing.item_spacing.y =
                1.0 - styles::spacing::app_item_gap_y::EGUI_POINTS;
            egui::ScrollArea::vertical()
                .id_salt("session-list")
                .auto_shrink([false, false])
                .max_height(list_height)
                .show(ui, |ui| {
                    ui.style_mut().spacing.item_spacing.y = 0.0;
                    let has_sessions = !self.available_sessions.is_empty();
                    let selected_index = self.selected_session.as_ref().and_then(|selected| {
                        self.available_sessions
                            .iter()
                            .position(|session| session == selected)
                    });
                    let create_response = create_session_row(ui, has_sessions, true);
                    if ((self.focus_on_entry && self.available_sessions.is_empty())
                        || self.focus_create_on_entry)
                        && create_response.enabled()
                    {
                        create_response.request_focus();
                        self.focus_on_entry = false;
                        self.focus_create_on_entry = false;
                    }
                    if create_response.clicked() {
                        self.new_session_error = None;
                        self.mode = SessionPageMode::CreateNew;
                        self.focus_session_input_on_entry = true;
                        self.focus_on_entry = false;
                        self.focus_create_on_entry = false;
                    }
                    let mut start = false;
                    let list_output = show_roving_list(
                        ui,
                        ui.id().with("session-rows"),
                        &self.available_sessions,
                        selected_index,
                        &mut self.focused_session,
                        self.focus_on_entry,
                        |ui, _index, name, selected, focusable| {
                            session_row(
                                ui,
                                name,
                                selected,
                                _index + 1 < self.available_sessions.len(),
                                focusable,
                            )
                        },
                    );
                    if list_output.focus_requested {
                        self.focus_on_entry = false;
                    }
                    if let Some(index) = list_output.selection_changed {
                        self.selected_session = Some(self.available_sessions[index].clone());
                    }
                    if let Some(index) = list_output.activated {
                        self.selected_session = Some(self.available_sessions[index].clone());
                        self.focused_session.focused_index = Some(index);
                        start = true;
                    }
                    if start {
                        self.start_existing_session(ui.ctx().clone());
                    }
                });
        });
    }

    fn show_create_form(&mut self, ui: &mut egui::Ui) {
        let is_submitting = self.create_session_task.is_pending();
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;

            if let Some(error) = self.new_session_error.as_deref() {
                display_error(ui, error);
                ui.add_space(styles::spacing::login_error_gap::EGUI_POINTS);
            }

            let submit_from_input = ui
                .add_enabled_ui(!is_submitting, |ui| {
                    let response = labeled_text_input(
                        ui,
                        "Session name",
                        TextEdit::singleline(&mut self.new_session_input)
                            .font(styles::typography::mono::font_id()),
                    );
                    if self.focus_session_input_on_entry && response.enabled() {
                        response.request_focus();
                        self.focus_session_input_on_entry = false;
                    }
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
                })
                .inner;

            ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
            ui.horizontal(|ui| {
                let submit = if is_submitting {
                    loading_button(
                        ui,
                        "Creating and starting…",
                        egui::Vec2::ZERO,
                        ButtonVariant::Primary,
                    )
                } else {
                    ui.add(primary_button("Create Session", ui))
                };
                let cancel = ui.add_enabled(!is_submitting, secondary_button("Cancel", ui));
                if cancel.clicked() {
                    self.new_session_error = None;
                    self.mode = SessionPageMode::SelectExisting;
                }
                if submit.clicked() || submit_from_input {
                    self.start_new_session(ui.ctx().clone());
                }
            });
        });
    }

    fn start_existing_session(&mut self, ctx: egui::Context) {
        if self.create_ingest_task.is_pending() {
            return;
        }
        if let Some(session) = self.selected_session.clone() {
            let task = CreateIngestRequest {
                project: self.project.clone(),
                session,
            };
            self.create_ingest_task.spawn(move || task.execute(), ctx);
        }
    }

    fn start_new_session(&mut self, ctx: egui::Context) {
        if self.create_session_task.is_pending() {
            return;
        }
        self.new_session_error = None;
        match validate_session_name(&self.new_session_input) {
            Ok(session) => {
                self.new_session_input.clone_from(&session);
                let task = CreateSessionAndIngestRequest {
                    project: self.project.clone(),
                    session,
                };
                self.create_session_task.spawn(move || task.execute(), ctx);
            }
            Err(error) => self.new_session_error = Some(error),
        }
    }

    pub fn poll(&mut self) -> Action {
        if let Some(sessions) = self.sessions_task.poll().take_ready() {
            self.available_sessions = sessions;
            self.initial_fetch_complete = true;
        }
        if let Some(active_ingest) = self.create_ingest_task.poll().take_ready() {
            return Action::SelectSession {
                session: active_ingest,
            };
        }
        if let Some(result) = self.create_session_task.poll().take_ready() {
            match result {
                Ok(active_ingest) => {
                    return Action::SelectSession {
                        session: active_ingest,
                    };
                }
                Err(error) => self.new_session_error = Some(error),
            }
        }
        Action::None
    }

    pub fn is_busy(&self) -> bool {
        self.sessions_task.is_pending()
            || self.create_ingest_task.is_pending()
            || self.create_session_task.is_pending()
            || (!self.initial_fetch_complete
                && matches!(self.sessions_task, AsyncState::Idle)
                && self.available_sessions.is_empty())
    }
}

fn create_session_row(ui: &mut egui::Ui, show_separator: bool, focusable: bool) -> egui::Response {
    interactive_row(
        ui,
        false,
        show_separator,
        focusable,
        |response, ui, _selected| {
            let label = "Create a new session";
            let mut job = LayoutJob::default();
            job.append(
                egui_phosphor::regular::PLUS,
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(
                        styles::typography::body::FONT_SIZE,
                        FontFamily::Proportional,
                    ),
                    color: styles::color::on_surface_dim::VALUE.resolve(ui.visuals()),
                    ..Default::default()
                },
            );
            job.append(
                label,
                4.0,
                egui::TextFormat {
                    font_id: styles::typography::body::font_id(),
                    color: styles::color::on_surface_dim::VALUE.resolve(ui.visuals()),
                    ..Default::default()
                },
            );

            let row_height = ui.fonts_mut(|f| f.row_height(&styles::typography::body::font_id()));
            let padding = (SESSION_ROW_HEIGHT - row_height) / 2.0;

            ui.vertical(|ui| {
                ui.add_space(padding);
                ui.add(Label::new(job).sense(Sense::empty()).selectable(false));
                ui.add_space(padding);
            });

            response
                .widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
        },
    )
}

fn session_row(
    ui: &mut egui::Ui,
    name: &str,
    selected: bool,
    show_separator: bool,
    focusable: bool,
) -> egui::Response {
    interactive_row(
        ui,
        selected,
        show_separator,
        focusable,
        |response, ui, selected| {
            let row_height = ui.fonts_mut(|f| f.row_height(&styles::typography::mono::font_id()));
            let padding = (SESSION_ROW_HEIGHT - row_height) / 2.0;

            ui.vertical(|ui| {
                ui.add_space(padding);
                ui.add(Label::new(name).sense(Sense::empty()).selectable(false));
                ui.add_space(padding);
            });

            response.widget_info(|| {
                WidgetInfo::selected(WidgetType::SelectableLabel, ui.is_enabled(), selected, name)
            });
        },
    )
}

fn interactive_row(
    ui: &mut egui::Ui,
    selected: bool,
    show_separator: bool,
    focusable: bool,
    content: impl Fn(&Response, &mut egui::Ui, bool),
) -> egui::Response {
    let row_size = egui::vec2(ui.available_width(), SESSION_ROW_HEIGHT);

    let (rect, response) = ui.allocate_exact_size(
        row_size,
        if focusable {
            Sense::click()
        } else {
            Sense::CLICK
        },
    );
    let content_rect = rect.shrink2(egui::vec2(
        styles::spacing::button_padding_x::EGUI_POINTS,
        0.0,
    ));

    let visuals = ui.visuals();
    let fill = if selected {
        styles::color::primary_tint::VALUE.resolve(visuals)
    } else if response.hovered() {
        styles::color::surface_variant::VALUE.resolve(visuals)
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    if show_separator {
        let sep_rect = &content_rect;
        ui.painter().hline(
            sep_rect.x_range(),
            sep_rect.bottom() - 1.0,
            Stroke::new(
                styles::components::standard_border::width::EGUI_POINTS,
                styles::color::outline::VALUE.resolve(visuals),
            ),
        );
    }

    let mut row_ui = ui.new_child(egui::UiBuilder::default().max_rect(content_rect));

    content(&response, &mut row_ui, selected);

    styles::paint_focus_rect(ui, &response);
    styles::pointer_cursor(response)
}

fn session_results_layout_style() -> Style {
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

fn session_list_layout_style() -> Style {
    let frame_vertical_padding = 4.0 * styles::spacing::button_padding_y::EGUI_POINTS;
    let frame_border = 2.0 * styles::components::standard_border::width::EGUI_POINTS;
    Style {
        flex_grow: 1.0,
        flex_shrink: 1.0,
        flex_basis: length(0.0_f32),
        min_size: taffy::Size {
            width: auto(),
            height: length(SESSION_ROW_HEIGHT + frame_vertical_padding + frame_border),
        },
        ..Default::default()
    }
}

fn session_action_layout_style() -> Style {
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

fn centered_loading(ui: &mut egui::Ui, height: f32) {
    let loading_text = "Fetching sessions…";
    let text_width = ui
        .painter()
        .layout_no_wrap(
            loading_text.to_owned(),
            styles::typography::body::font_id(),
            ui.visuals().text_color(),
        )
        .size()
        .x;
    let content_width = ui.spacing().interact_size.y + ui.spacing().item_spacing.x + text_width;
    let available = ui.available_rect_before_wrap();
    let (_, reserved) = ui.allocate_space(egui::vec2(available.width(), height));
    let content_rect = egui::Rect::from_center_size(
        reserved.center(),
        egui::vec2(content_width, ui.spacing().interact_size.y),
    );
    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    content_ui.spinner();
    content_ui.label(RichText::new(loading_text).font(styles::typography::body::font_id()));
}

fn validate_session_name(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Err("Please enter a session name.".to_owned())
    } else if trimmed.contains('/') {
        Err("Session name cannot contain a \"/\".".to_owned())
    } else {
        Ok(trimmed.to_owned())
    }
}

fn today() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

fn session_name_prefix(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}-",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn content_guide_widget_offset(horizontal_padding: f32) -> f32 {
    (styles::content_card_inset() - horizontal_padding).max(0.0)
}

#[cfg(test)]
mod tests {
    use time::{Date, Month};

    use super::*;

    #[test]
    fn test_fetch_sessions_request() {
        let req = FetchSessionsRequest {
            project: "test".to_string(),
        };
        let res = req.execute();
        assert_eq!(res.len(), 4);
    }

    #[test]
    fn formats_session_name_prefix() {
        let date = Date::from_calendar_date(2026, Month::January, 3).unwrap();
        assert_eq!(session_name_prefix(date), "2026-01-03-");
    }

    #[test]
    fn validates_session_names() {
        assert_eq!(
            validate_session_name("  2026-07-30-run  "),
            Ok("2026-07-30-run".to_owned())
        );
        assert_eq!(
            validate_session_name("  "),
            Err("Please enter a session name.".to_owned())
        );
        assert_eq!(
            validate_session_name("2026-07-30/run"),
            Err("Session name cannot contain a \"/\".".to_owned())
        );
    }

    #[test]
    fn create_request_can_simulate_an_api_rejection() {
        let rejected = CreateSessionAndIngestRequest {
            project: "test".to_owned(),
            session: "2026-07-30-FORBIDDEN-run".to_owned(),
        }
        .execute();
        assert_eq!(rejected, Err(ASYNC_REJECTION_MESSAGE.to_owned()));
    }

    #[test]
    fn create_request_starts_ingest_after_success() {
        let result = CreateSessionAndIngestRequest {
            project: "test".to_owned(),
            session: "2026-07-30-run".to_owned(),
        }
        .execute()
        .unwrap();
        assert_eq!(result.project, "test");
        assert_eq!(result.session, "2026-07-30-run");
    }
}
