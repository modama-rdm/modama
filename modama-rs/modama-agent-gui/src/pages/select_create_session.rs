use std::sync::Arc;

use eframe::egui::{
    self, Align, FontFamily, FontId, Label, Layout, Response, RichText, Sense, Stroke, TextEdit,
    WidgetInfo, WidgetType, text::LayoutJob,
};
use egui_taffy::{
    TuiBuilderLogic, TuiContainerResponse,
    taffy::{
        self,
        prelude::{AlignItems, FlexDirection, FlexWrap, Position, Style, auto, length, percent},
    },
    tui,
};
use time::{Date, OffsetDateTime};

use crate::{
    actions::{Action, RetryAction, TaskFailure},
    async_state::AsyncState,
    components::{
        buttons::{ButtonVariant, loading_button, primary_button, secondary_button},
        dialogs::{ChoiceDialogAction, DialogChoice, choice_dialog},
        display_error,
        flow_navigation::{FlowStage, flow_navigation},
        forms::labeled_text_input,
        list_view::{RovingListOutput, RovingListState, show_roving_list},
        page_heading,
    },
    service::{AgentService, ServiceError, StubAgentService},
    state::{OperationId, PendingSession, SignedIn},
    styles,
};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CreateSessionError {
    #[error("Please enter a session name.")]
    EmptyName,
    #[error(
        "Session names can only contain letters A-Z, digits, spaces, \".\", \"_\" and \"-\", \
         not \"{0}\"."
    )]
    InvalidCharacter(char),
    #[error("Session names can be up to {MAX_SESSION_NAME_LENGTH} characters long.")]
    TooLong,
    #[error("{0}")]
    Server(String),
    #[error("Session was created, but the ingest share could not be started: {0}")]
    Start(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SessionPageMode {
    #[default]
    SelectExisting,
    CreateNew,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SessionFocusTarget {
    #[default]
    None,
    ExistingList,
    CreateRow,
    SessionName,
}

pub struct PageSelectCreateSession {
    pub project: String,
    pub selected_session: Option<String>,
    pub new_session_input: String,
    pub create_session_error: Option<CreateSessionError>,
    pub sessions_task: AsyncState<Result<Vec<String>, ServiceError>>,
    pub create_ingest_task: AsyncState<Result<PendingSession, ServiceError>>,
    pub create_session_task: AsyncState<Result<PendingSession, CreateSessionError>>,
    pub available_sessions: Vec<String>,
    pub session_date: Date,
    mode: SessionPageMode,
    focused_session: RovingListState,
    focus_target: SessionFocusTarget,
    service: Arc<dyn AgentService>,
    signed_in: SignedIn,
    /// Operation identity paired with the public async task fields in test fixtures.
    pub start_operation_id: Option<OperationId>,
    started_action: Option<Box<Action>>,
    /// The existing session the user asked to create, while the page asks whether to ingest
    /// into it
    existing_session_confirmation: Option<String>,
    focus_existing_cancel: bool,
}

const EXISTING_SESSION_DIALOG_ID: &str = "existing-session-confirmation";

impl PageSelectCreateSession {
    pub fn new(project: String) -> Self {
        Self::with_service(
            project,
            StubAgentService::shared(),
            SignedIn {
                username: "representative.user".to_owned(),
            },
        )
    }

    pub fn with_service(
        project: String,
        service: Arc<dyn AgentService>,
        signed_in: SignedIn,
    ) -> Self {
        let session_date = today();
        Self {
            project,
            selected_session: None,
            new_session_input: session_name_prefix(session_date),
            create_session_error: None,
            sessions_task: AsyncState::Idle,
            create_ingest_task: AsyncState::Idle,
            create_session_task: AsyncState::Idle,
            available_sessions: Vec::new(),
            session_date,
            mode: SessionPageMode::SelectExisting,
            focused_session: RovingListState::default(),
            focus_target: SessionFocusTarget::ExistingList,
            service,
            signed_in,
            start_operation_id: None,
            started_action: None,
            existing_session_confirmation: None,
            focus_existing_cancel: false,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        self.handle_keyboard_shortcuts(ui);
        if self.sessions_task.is_idle() && self.available_sessions.is_empty() {
            let project = self.project.clone();
            let service = Arc::clone(&self.service);
            let signed_in = self.signed_in.clone();
            let ctx = ui.ctx().clone();
            self.sessions_task
                .spawn(move || service.list_sessions(&signed_in, &project), ctx);
        }

        if let Some(stage) = flow_navigation(ui, FlowStage::Session, !self.is_busy()) {
            return match stage {
                FlowStage::Overview => Action::GoToOverview,
                FlowStage::Project => Action::GoToSelectProject,
                FlowStage::Session => Action::None,
            };
        }
        ui.add_space(styles::spacing::header_title_gap::EGUI_POINTS);
        page_heading(
            ui,
            &format!("Select or create a session in project '{}'", self.project),
        );
        ui.add_space(styles::content_card_top_margin_adjustment());

        match self.mode {
            SessionPageMode::SelectExisting => self.show_session_selection(ui),
            SessionPageMode::CreateNew => self.show_create_form(ui),
        }
        self.show_existing_session_confirmation(ui);

        self.take_started_action()
    }

    fn handle_keyboard_shortcuts(&mut self, ui: &mut egui::Ui) {
        if self.existing_session_confirmation.is_some() {
            // the dialog handles its keys
            return;
        }
        let command_n =
            ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::N));
        if self.mode == SessionPageMode::SelectExisting
            && command_n
            && !self.sessions_task.is_pending()
            && !self.create_ingest_task.is_pending()
        {
            self.enter_create_mode();
            return;
        }

        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && self.mode == SessionPageMode::CreateNew
            && !self.create_session_task.is_pending()
        {
            self.return_to_selection(true);
        }
    }

    fn enter_create_mode(&mut self) {
        self.create_session_error = None;
        self.mode = SessionPageMode::CreateNew;
        self.focus_target = SessionFocusTarget::SessionName;
    }

    fn return_to_selection(&mut self, focus_create_row: bool) {
        self.create_session_error = None;
        self.mode = SessionPageMode::SelectExisting;
        self.focus_target = if focus_create_row {
            SessionFocusTarget::CreateRow
        } else {
            SessionFocusTarget::None
        };
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
        let list_height = (outer_height - frame_vertical_padding - frame_border)
            .max(styles::components::session_row::height::EGUI_POINTS);
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
            styles::scrollbar_scope(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("session-list")
                    .auto_shrink([false, false])
                    .max_height(list_height)
                    .show(ui, |ui| {
                        ui.style_mut().spacing.item_spacing.y = 0.0;
                        let selected_index = self.selected_session_index();
                        let create_response = create_session_row(ui, true);
                        if ((self.focus_target == SessionFocusTarget::ExistingList
                            && self.available_sessions.is_empty())
                            || self.focus_target == SessionFocusTarget::CreateRow)
                            && create_response.enabled()
                        {
                            create_response.request_focus();
                            self.focus_target = SessionFocusTarget::None;
                        }
                        if create_response.clicked() {
                            self.enter_create_mode();
                        }
                        let list_output = show_roving_list(
                            ui,
                            ui.id().with("session-rows"),
                            &self.available_sessions,
                            selected_index,
                            &mut self.focused_session,
                            self.focus_target == SessionFocusTarget::ExistingList,
                            |ui, _, name, selected, focusable| {
                                session_row(ui, name, selected, true, focusable)
                            },
                        );
                        self.apply_session_list_output(list_output, ui.ctx().clone());
                    });
            });
        });
    }

    fn selected_session_index(&self) -> Option<usize> {
        self.selected_session.as_ref().and_then(|selected| {
            self.available_sessions
                .iter()
                .position(|session| session == selected)
        })
    }

    fn apply_session_list_output(&mut self, output: RovingListOutput, ctx: egui::Context) {
        if output.focus_requested {
            self.focus_target = SessionFocusTarget::None;
        }
        if let Some(index) = output.selection_changed {
            self.selected_session = Some(self.available_sessions[index].clone());
        }
        if let Some(index) = output.activated {
            self.selected_session = Some(self.available_sessions[index].clone());
            self.focused_session.focused_index = Some(index);
            self.start_existing_session(ctx);
        }
    }

    fn show_create_form(&mut self, ui: &mut egui::Ui) {
        let is_submitting = self.create_session_task.is_pending();
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;

            if let Some(error) = self.create_session_error.as_ref() {
                display_error(ui, &error.to_string());
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
                    if self.focus_target == SessionFocusTarget::SessionName && response.enabled() {
                        response.request_focus();
                        self.focus_target = SessionFocusTarget::None;
                    }
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
                })
                .inner;

            ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
            session_creation_hint(ui, self.session_date);
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
                    self.return_to_selection(false);
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
            let project = self.project.clone();
            self.record_start(project.clone(), session.clone());
            let service = Arc::clone(&self.service);
            let signed_in = self.signed_in.clone();
            self.create_ingest_task.spawn(
                move || service.start_ingest(&signed_in, &project, &session),
                ctx,
            );
        }
    }

    fn start_new_session(&mut self, ctx: egui::Context) {
        if self.create_session_task.is_pending() || self.existing_session_confirmation.is_some() {
            return;
        }
        let name = self.new_session_input.trim();
        if self
            .available_sessions
            .iter()
            .any(|session| session == name)
        {
            self.existing_session_confirmation = Some(name.to_owned());
            self.focus_existing_cancel = true;
            return;
        }
        self.spawn_create_session_task(self.new_session_input.clone(), ctx);
    }

    fn show_existing_session_confirmation(&mut self, ui: &mut egui::Ui) {
        let Some(session) = self.existing_session_confirmation.clone() else {
            return;
        };
        let message = format!(
            "The session {session} exists already in project {}. Start the ingest into it?",
            self.project
        );
        let action = choice_dialog(
            ui.ctx(),
            ui.visuals(),
            EXISTING_SESSION_DIALOG_ID,
            "Session exists already",
            &message,
            &[DialogChoice {
                label: "Use existing session",
                variant: ButtonVariant::Primary,
                value: (),
            }],
            self.focus_existing_cancel,
        );
        self.focus_existing_cancel = false;
        match action {
            ChoiceDialogAction::None => {}
            ChoiceDialogAction::Cancel => {
                self.existing_session_confirmation = None;
                self.focus_target = SessionFocusTarget::SessionName;
            }
            ChoiceDialogAction::Choose(()) => {
                self.existing_session_confirmation = None;
                self.spawn_create_session_task(session, ui.ctx().clone());
            }
        }
    }

    pub fn poll(&mut self) -> Action {
        match self.sessions_task.poll() {
            Ok(task) => {
                if let Some(result) = task.take_ready() {
                    match result {
                        Ok(sessions) => self.available_sessions = sessions,
                        Err(error) => {
                            log::error!("session-fetch failed: {error}");
                            return Action::TaskFailed(TaskFailure::ReturnToOverview {
                                message: "Could not load sessions.".to_owned(),
                                retry: RetryAction::FetchSessions {
                                    project: self.project.clone(),
                                },
                            });
                        }
                    }
                }
            }
            Err(error) => {
                log::error!("session-fetch task failed: {error}");
                return Action::TaskFailed(TaskFailure::ReturnToOverview {
                    message: "Could not load sessions.".to_owned(),
                    retry: RetryAction::FetchSessions {
                        project: self.project.clone(),
                    },
                });
            }
        }
        match self.create_ingest_task.poll() {
            Ok(task) => {
                if let Some(result) = task.take_ready() {
                    match result {
                        Ok(pending_ingest) => {
                            return self.start_succeeded(pending_ingest);
                        }
                        Err(error) => {
                            log::error!("create-ingest failed: {error}");
                            let session = self
                                .selected_session
                                .clone()
                                .unwrap_or_else(|| self.new_session_input.clone());
                            return self.start_failed(Some(TaskFailure::Stay {
                                message: "Could not start the ingest share.".to_owned(),
                                retry: RetryAction::CreateIngest {
                                    project: self.project.clone(),
                                    session,
                                },
                            }));
                        }
                    }
                }
            }
            Err(error) => {
                log::error!("create-ingest task failed: {error}");
                if let Some(session) = self.selected_session.clone() {
                    return self.start_failed(Some(TaskFailure::Stay {
                        message: "Could not start the ingest share.".to_owned(),
                        retry: RetryAction::CreateIngest {
                            project: self.project.clone(),
                            session,
                        },
                    }));
                }
                return self.start_failed(Some(TaskFailure::Stay {
                    message: "Could not start the ingest share.".to_owned(),
                    retry: RetryAction::CreateSession {
                        project: self.project.clone(),
                        session: self.new_session_input.clone(),
                    },
                }));
            }
        }
        match self.create_session_task.poll() {
            Ok(task) => {
                if let Some(result) = task.take_ready() {
                    match result {
                        Ok(pending_ingest) => {
                            return self.start_succeeded(pending_ingest);
                        }
                        Err(error @ CreateSessionError::Start(_)) => {
                            log::error!("create-ingest after session creation failed: {error}");
                            return self.start_failed(Some(TaskFailure::Stay {
                                    message: "The session was created, but its ingest share could not be started.".to_owned(),
                                    retry: RetryAction::CreateIngest { project: self.project.clone(), session: self.new_session_input.trim().to_owned() },
                                }));
                        }
                        Err(error @ CreateSessionError::Server(_)) => {
                            log::error!("create-session failed: {error}");
                            self.create_session_error = Some(error);
                            return self.start_failed(None);
                        }
                        Err(error) => {
                            self.create_session_error = Some(error);
                            return self.start_failed(None);
                        }
                    }
                }
            }
            Err(error) => {
                log::error!("create-session task failed: {error}");
                return self.start_failed(Some(TaskFailure::Stay {
                    message: "Could not create the session.".to_owned(),
                    retry: RetryAction::CreateSession {
                        project: self.project.clone(),
                        session: self.new_session_input.clone(),
                    },
                }));
            }
        }
        Action::None
    }

    pub fn retry_create_ingest(
        &mut self,
        project: String,
        session: String,
        ctx: egui::Context,
    ) -> Action {
        self.selected_session = Some(session.clone());
        self.create_ingest_task.reset();
        self.record_start(project.clone(), session.clone());
        let service = Arc::clone(&self.service);
        let signed_in = self.signed_in.clone();
        self.create_ingest_task.spawn(
            move || service.start_ingest(&signed_in, &project, &session),
            ctx,
        );
        self.take_started_action()
    }

    pub fn retry_create_session(
        &mut self,
        project: String,
        session: String,
        ctx: egui::Context,
    ) -> Action {
        self.new_session_input = session.clone();
        self.spawn_create_session_task_for_project(project, session, ctx);
        self.take_started_action()
    }

    fn spawn_create_session_task(&mut self, session: String, ctx: egui::Context) {
        self.spawn_create_session_task_for_project(self.project.clone(), session, ctx);
    }

    fn spawn_create_session_task_for_project(
        &mut self,
        project: String,
        session: String,
        ctx: egui::Context,
    ) {
        self.create_session_error = None;
        let session = match validate_session_name(&session) {
            Ok(session) => session,
            Err(error) => {
                self.create_session_error = Some(error);
                return;
            }
        };
        self.record_start(project.clone(), session.clone());
        self.create_session_task.reset();
        let service = Arc::clone(&self.service);
        let signed_in = self.signed_in.clone();
        self.create_session_task.spawn(
            move || {
                service
                    .create_session(&signed_in, &project, &session)
                    .map_err(|error| CreateSessionError::Server(refusal_message(error)))?;
                service
                    .start_ingest(&signed_in, &project, &session)
                    .map_err(|error| CreateSessionError::Start(error.to_string()))
            },
            ctx,
        );
    }

    fn record_start(&mut self, project: String, session: String) {
        let operation_id = OperationId::next();
        self.start_operation_id = Some(operation_id);
        self.started_action = Some(Box::new(Action::StartRequested {
            operation_id,
            project,
            session,
        }));
    }

    fn take_started_action(&mut self) -> Action {
        self.started_action
            .take()
            .map_or(Action::None, |action| *action)
    }

    fn start_succeeded(&mut self, session: PendingSession) -> Action {
        let Some(operation_id) = self.start_operation_id.take() else {
            log::error!("start task completed without an operation ID");
            return Action::None;
        };
        Action::IngestCreated {
            operation_id,
            session,
        }
    }

    fn start_failed(&mut self, failure: Option<TaskFailure>) -> Action {
        let Some(operation_id) = self.start_operation_id.take() else {
            log::error!("start task failed without an operation ID");
            return Action::None;
        };
        Action::StartFailed {
            operation_id,
            failure,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.sessions_task.is_pending()
            || self.create_ingest_task.is_pending()
            || self.create_session_task.is_pending()
            || (self.sessions_task.is_idle() && self.available_sessions.is_empty())
    }
}

fn create_session_row(ui: &mut egui::Ui, focusable: bool) -> egui::Response {
    interactive_row(ui, false, false, focusable, |response, ui, _selected| {
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
            styles::spacing::unit::EGUI_POINTS,
            egui::TextFormat {
                font_id: styles::typography::body::font_id(),
                color: styles::color::on_surface_dim::VALUE.resolve(ui.visuals()),
                ..Default::default()
            },
        );

        let row_height = ui.fonts_mut(|f| f.row_height(&styles::typography::body::font_id()));
        let padding = (styles::components::session_row::height::EGUI_POINTS - row_height) / 2.0;

        ui.vertical(|ui| {
            ui.add_space(padding);
            ui.add(Label::new(job).sense(Sense::empty()).selectable(false));
            ui.add_space(padding);
        });

        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    })
}

fn session_row(
    ui: &mut egui::Ui,
    name: &str,
    selected: bool,
    show_top_separator: bool,
    focusable: bool,
) -> egui::Response {
    interactive_row(
        ui,
        selected,
        show_top_separator,
        focusable,
        |response, ui, selected| {
            let row_height = ui.fonts_mut(|f| f.row_height(&styles::typography::mono::font_id()));
            let padding = (styles::components::session_row::height::EGUI_POINTS - row_height) / 2.0;

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
    show_top_separator: bool,
    focusable: bool,
    content: impl Fn(&Response, &mut egui::Ui, bool),
) -> egui::Response {
    let row_size = egui::vec2(
        ui.available_width(),
        styles::components::session_row::height::EGUI_POINTS,
    );

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
    if show_top_separator {
        ui.painter().hline(
            content_rect.x_range(),
            content_rect.top(),
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
            height: length(
                styles::components::session_row::height::EGUI_POINTS
                    + frame_vertical_padding
                    + frame_border,
            ),
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

/// What to show for a failed request: a refusal (4xx) in the server's words, without the
/// status code; anything else as the error says it
fn refusal_message(error: ServiceError) -> String {
    match error {
        ServiceError::Server { status, message } if (400..500).contains(&status) => message,
        error => error.to_string(),
    }
}

/// The management API's limit for a session name (`CreateSessionRequest.name`)
const MAX_SESSION_NAME_LENGTH: usize = 255;

/// The management API's rule for session names (`CreateSessionRequest.name` in its OpenAPI
/// document: `^[A-Za-z0-9 ._-]+$`, up to 255 characters), checked before sending, so the user
/// gets a sentence instead of the API's refusal
fn validate_session_name(input: &str) -> Result<String, CreateSessionError> {
    let trimmed = input.trim();
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-');
    if trimmed.is_empty() {
        Err(CreateSessionError::EmptyName)
    } else if let Some(invalid) = trimmed.chars().find(|c| !allowed(*c)) {
        Err(CreateSessionError::InvalidCharacter(invalid))
    } else if trimmed.chars().count() > MAX_SESSION_NAME_LENGTH {
        Err(CreateSessionError::TooLong)
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

fn session_name_example(date: Date) -> String {
    format!("{}alignment-run", session_name_prefix(date))
}

fn session_creation_hint(ui: &mut egui::Ui, session_date: Date) {
    ui.label(
        RichText::new(format!(
            "Store measurement data and important notes in the raw subfolder. Today's date is pre-filled; add a suffix, e.g. {}.",
            session_name_example(session_date),
        ))
        .font(styles::typography::supporting_text::font_id())
        .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
    );
}

fn content_guide_widget_offset(horizontal_padding: f32) -> f32 {
    (styles::guide_inset() - horizontal_padding).max(0.0)
}

#[cfg(test)]
mod tests {
    use time::{Date, Month};

    use super::*;

    #[test]
    fn formats_session_name_prefix() {
        let date = Date::from_calendar_date(2026, Month::January, 3).unwrap();
        assert_eq!(session_name_prefix(date), "2026-01-03-");
    }

    #[test]
    fn formats_session_name_example_with_the_date_prefix() {
        let date = Date::from_calendar_date(2026, Month::January, 3).unwrap();
        assert_eq!(session_name_example(date), "2026-01-03-alignment-run");
    }

    #[test]
    fn validates_session_names() {
        assert_eq!(
            validate_session_name("  2026-07-30-run  "),
            Ok("2026-07-30-run".to_owned())
        );
        assert_eq!(
            validate_session_name("  "),
            Err(CreateSessionError::EmptyName)
        );
        assert_eq!(
            validate_session_name("2026-07-30/run"),
            Err(CreateSessionError::InvalidCharacter('/'))
        );
        assert_eq!(
            validate_session_name("2026-11-10-second run Müller"),
            Err(CreateSessionError::InvalidCharacter('ü'))
        );
        assert_eq!(
            validate_session_name("2026-11-10 run_2.b-c"),
            Ok("2026-11-10 run_2.b-c".to_owned())
        );
        assert_eq!(
            validate_session_name(&"a".repeat(MAX_SESSION_NAME_LENGTH)),
            Ok("a".repeat(MAX_SESSION_NAME_LENGTH))
        );
        assert_eq!(
            validate_session_name(&"a".repeat(MAX_SESSION_NAME_LENGTH + 1)),
            Err(CreateSessionError::TooLong)
        );
    }

    #[test]
    fn a_refusal_is_shown_in_the_servers_words() {
        let refused = ServiceError::Server {
            status: 422,
            message: "name: String should match pattern".to_owned(),
        };
        assert_eq!(
            refusal_message(refused),
            "name: String should match pattern"
        );
        let failed = ServiceError::Server {
            status: 502,
            message: "unreachable".to_owned(),
        };
        assert_eq!(refusal_message(failed), "server error (502): unreachable");
    }

    #[test]
    fn invalid_characters_are_named_in_the_message() {
        assert_eq!(
            CreateSessionError::InvalidCharacter('ü').to_string(),
            "Session names can only contain letters A-Z, digits, spaces, \".\", \"_\" and \"-\", \
             not \"ü\"."
        );
    }

    #[test]
    fn create_session_server_failures_remain_visible_inline() {
        let mut page = PageSelectCreateSession::new("Holo Stacks".to_owned());
        let operation_id = OperationId::next();
        page.start_operation_id = Some(operation_id);
        page.create_session_task = AsyncState::Ready {
            payload: Err(CreateSessionError::Server("request rejected".to_owned())),
        };

        assert_eq!(
            page.poll(),
            Action::StartFailed {
                operation_id,
                failure: None,
            }
        );
        assert_eq!(
            page.create_session_error,
            Some(CreateSessionError::Server("request rejected".to_owned()))
        );
    }

    #[test]
    fn start_failure_after_creation_retains_the_ingest_retry() {
        let mut page = PageSelectCreateSession::new("Holo Stacks".to_owned());
        page.new_session_input = "  2026-08-06-alignment-run  ".to_owned();
        let operation_id = OperationId::next();
        page.start_operation_id = Some(operation_id);
        page.create_session_task = AsyncState::Ready {
            payload: Err(CreateSessionError::Start("mount failed".to_owned())),
        };

        assert_eq!(
            page.poll(),
            Action::StartFailed {
                operation_id,
                failure: Some(TaskFailure::Stay {
                    message: "The session was created, but its ingest share could not be started."
                        .to_owned(),
                    retry: RetryAction::CreateIngest {
                        project: "Holo Stacks".to_owned(),
                        session: "2026-08-06-alignment-run".to_owned(),
                    },
                }),
            }
        );
    }
}
