use eframe::egui::{self, Align, Label, Layout, RichText};
use modama_common::{
    internal_api_types::{ConnectedAgent, ConnectedAgents},
    messages::AgentMountStatus,
};
use std::sync::Arc;

use crate::{
    actions::{Action, LoginDestination, RetryAction, TaskFailure},
    async_state::AsyncState,
    components::{
        buttons::{
            ButtonVariant, danger_outline_button, loading_button, primary_button, secondary_button,
        },
        copy_field::{copy_field, icon_font},
        detail_label,
        dialogs::{
            ChoiceDialogAction, ConfirmationDialogAction, DialogChoice, choice_dialog,
            destructive_confirmation_dialog,
        },
        page_heading,
        path_line::path_line,
    },
    notebook_snippet::python_snippet,
    service::{AgentService, ServiceError, StubAgentService},
    state::{
        ActiveSession, AgentAppState, ForegroundSession, OperationId, PendingSession, SignedIn,
        instrument_name,
    },
    styles,
};

const CLOSE_CONFIRMATION_ID: &str = "close-session-confirmation";
const START_DIALOG_ID: &str = "start-session-while-open";
const PATHS_DIALOG_ID: &str = "mount-path-and-code";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartChoice {
    KeepOpen,
    ClosePrevious,
    Continue,
}

/// What to ask before starting a session while a share is open
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StartDialog {
    /// The user's own share (or one whose creator is unknown): keep it, or close it afterwards
    OwnShare { previous: ActiveSession },
    /// Someone else's: starting is a hand-off, and their share is no longer available here
    OthersShare {
        project: String,
        session: String,
        user: String,
    },
}

pub(crate) fn start_dialog_kind(
    foreground: &ForegroundSession,
    signed_in: &SignedIn,
) -> Option<StartDialog> {
    let previous = match foreground {
        ForegroundSession::None => return None,
        ForegroundSession::Mounted(session) => session.clone(),
        ForegroundSession::Mounting(pending, _) => ActiveSession {
            share_id: pending.share_id.clone(),
            project: pending.project.clone(),
            session: pending.session.clone(),
            mount_path: None,
            created_by: pending.created_by.clone(),
        },
    };
    match others_share_user(previous.created_by.as_deref(), signed_in) {
        Some(user) => Some(StartDialog::OthersShare {
            project: previous.project.clone(),
            session: previous.session.clone(),
            user: user.to_owned(),
        }),
        None => Some(StartDialog::OwnShare { previous }),
    }
}

/// The user who started an ingest share, if that is not the signed-in user. A share whose
/// creator isn't known counts as the signed-in user's own.
pub(crate) fn others_share_user<'a>(
    created_by: Option<&'a str>,
    signed_in: &SignedIn,
) -> Option<&'a str> {
    created_by.filter(|user| *user != signed_in.username)
}

/// What starting another ingest does to the running ingest share, shown before the user
/// clicks;
/// None while signed out, when it depends on who signs in
fn start_hint(app_state: &AgentAppState) -> Option<String> {
    let signed_in = app_state.signed_in.as_ref()?;
    Some(
        match start_dialog_kind(&app_state.agent.foreground, signed_in)? {
            StartDialog::OwnShare { previous } if previous.created_by.is_some() => format!(
                "Your ingest share for session {} keeps running, unless you choose to stop it.",
                previous.session
            ),
            StartDialog::OwnShare { previous } => format!(
                "The ingest share for session {} keeps running, unless you choose to stop it.",
                previous.session
            ),
            StartDialog::OthersShare { session, user, .. } => format!(
                "{user}'s ingest share for session {session} will no longer be available at {}. \
             Please coordinate with {user} first.",
                instrument_name(app_state.identity.as_ref())
            ),
        },
    )
}

pub struct PageOverview {
    pub close_task: AsyncState<Result<(), ServiceError>>,
    close_confirmation_open: bool,
    focus_confirmation_cancel: bool,
    focus_on_entry: bool,
    service: Arc<dyn AgentService>,
    last_close: Option<(SignedIn, ActiveSession)>,
    close_operation_id: Option<OperationId>,
    start_dialog_open: bool,
    focus_start_cancel: bool,
    /// The user's previous share, to close on the first frame (after starting a new one)
    close_previous: Option<(SignedIn, ActiveSession)>,
    /// The share from the "Also mounted" list the close confirmation is for; None while it
    /// is for the foreground share
    close_mount: Option<ActiveSession>,
    /// The dialog with the mount path and the notebook code
    paths_dialog_open: bool,
    focus_paths_close: bool,
}

impl Default for PageOverview {
    fn default() -> Self {
        Self::new()
    }
}

impl PageOverview {
    pub fn new() -> Self {
        Self::with_service(StubAgentService::shared())
    }

    pub fn with_service(service: Arc<dyn AgentService>) -> Self {
        Self {
            close_task: AsyncState::Idle,
            close_confirmation_open: false,
            focus_confirmation_cancel: false,
            focus_on_entry: true,
            service,
            last_close: None,
            close_operation_id: None,
            start_dialog_open: false,
            focus_start_cancel: false,
            close_previous: None,
            close_mount: None,
            paths_dialog_open: false,
            focus_paths_close: false,
        }
    }

    /// The overview right after a new session was started, which closes the user's previous
    /// share as they asked
    pub fn closing_previous(
        service: Arc<dyn AgentService>,
        signed_in: SignedIn,
        previous: ActiveSession,
    ) -> Self {
        let mut page = Self::with_service(service);
        page.close_previous = Some((signed_in, previous));
        page
    }

    #[cfg(test)]
    pub(crate) fn closes_previous(&self) -> Option<&ActiveSession> {
        self.close_previous.as_ref().map(|(_, previous)| previous)
    }

    #[cfg(test)]
    pub(crate) fn closes_mount(&self) -> Option<&ActiveSession> {
        self.close_mount.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn start_dialog_is_open(&self) -> bool {
        self.start_dialog_open
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
        app_state: &AgentAppState,
    ) -> Action {
        page_heading(
            ui,
            &format!(
                "Ingest for {}",
                instrument_name(app_state.identity.as_ref())
            ),
        );
        // the scroll area clips at its edges: inside it, the first card's border needs room;
        // that room comes out of the gap above, so the cards stay where they were before
        let top_room = styles::components::standard_border::width::EGUI_POINTS;
        ui.add_space((styles::content_card_top_margin_adjustment() - top_room).max(0.0));

        // the user asked to close their previous share once the new one has started
        if let Some((signed_in, previous)) = self.close_previous.take() {
            self.last_close = Some((signed_in.clone(), previous.clone()));
            return self.start_close(signed_in, previous, ui.ctx().clone());
        }

        // the cards scroll below the fixed heading: with several shares they are taller than
        // the window. The scroll area runs over the shell's right and bottom margins up to the
        // window's edges, so its bar sits next to the cards and nothing is cut off above the
        // window's bottom edge; the margins come back as space around the cards
        let content_width = ui.available_width();
        let mut scroll_rect = ui.available_rect_before_wrap();
        // the item spacing before it is part of the margin too
        let bottom_room =
            (ui.clip_rect().max.y - scroll_rect.max.y - ui.spacing().item_spacing.y).max(0.0);
        scroll_rect.max = ui.clip_rect().max;
        let action = ui
            .scope_builder(egui::UiBuilder::new().max_rect(scroll_rect), |ui| {
                styles::scrollbar_scope(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("overview-cards")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_max_width(content_width);
                            ui.add_space(top_room);
                            let action = self.show_cards(ui, app_state);
                            ui.add_space(bottom_room);
                            action
                        })
                        .inner
                })
                .inner
            })
            .inner;
        if action != Action::None {
            return action;
        }

        if self.start_dialog_open {
            return self.show_start_dialog(ui, app_state);
        }

        Action::None
    }

    /// The session card, with the start action next to the session, and the other mounts
    fn show_cards(&mut self, ui: &mut egui::Ui, app_state: &AgentAppState) -> Action {
        let pending_start = app_state.pending_start();
        let resource = instrument_name(app_state.identity.as_ref());
        let mut action = Action::None;
        let mut start_clicked = false;
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(session) = pending_start {
                self.drop_foreground_close_confirmation();
                // a share being started has no creator on record yet; it's the user
                let started_by = session.created_by.as_deref().or(app_state
                    .signed_in
                    .as_ref()
                    .map(|signed_in| signed_in.username.as_str()));
                self.show_mounting_session(ui, session, started_by, "Waiting for agent…", false);
            } else if matches!(app_state.agent.foreground, ForegroundSession::None) {
                self.drop_foreground_close_confirmation();
                start_clicked = self.show_create_section(ui, resource, None);
                show_connected_agents(ui, app_state.agent.connected_agents.as_ref());
            } else {
                let mut columns = Columns::new(ui);
                action = self.show_foreground_section(columns.first(), app_state);
                let second = columns.second(ui);
                let hint = start_hint(app_state);
                start_clicked = self.show_create_section(second, resource, hint.as_deref());
                show_connected_agents(second, app_state.agent.connected_agents.as_ref());
                columns.finish(ui);
            }
        });
        if action != Action::None {
            return action;
        }
        let action = self.show_other_mounts(ui, app_state);
        if action != Action::None {
            return action;
        }
        if start_clicked {
            return self.start_clicked(app_state);
        }
        Action::None
    }

    /// The foreground share's half of the session card
    fn show_foreground_section(&mut self, ui: &mut egui::Ui, app_state: &AgentAppState) -> Action {
        let pending_close = app_state.pending_close();
        let signed_in = &app_state.signed_in;
        match &app_state.agent.foreground {
            ForegroundSession::Mounted(session) => {
                let others_share = signed_in.as_ref().is_some_and(|signed_in| {
                    others_share_user(session.created_by.as_deref(), signed_in).is_some()
                });
                if others_share {
                    self.drop_foreground_close_confirmation();
                }
                let close_clicked = self.show_active_session(
                    ui,
                    session,
                    pending_close == Some(&session.share_id),
                    others_share,
                );
                if close_clicked {
                    if signed_in.is_some() {
                        self.open_close_confirmation();
                    } else {
                        return Action::GoToLogin {
                            destination: LoginDestination::ConfirmClose {
                                share_id: session.share_id.clone(),
                            },
                        };
                    }
                }
                if self.close_confirmation_open && self.close_mount.is_none() {
                    if let Some(signed_in) = signed_in {
                        let action = self.show_close_confirmation(ui, session, signed_in);
                        if action != Action::None {
                            return action;
                        }
                    } else {
                        self.close_confirmation_open = false;
                        return Action::GoToLogin {
                            destination: LoginDestination::ConfirmClose {
                                share_id: session.share_id.clone(),
                            },
                        };
                    }
                }
            }
            ForegroundSession::Mounting(session, message) => {
                self.drop_foreground_close_confirmation();
                let started_by = session.created_by.as_deref();
                if self.show_mounting_session(
                    ui,
                    session,
                    started_by,
                    message,
                    app_state.agent.hand_off_blocked,
                ) {
                    return Action::ForceRelease;
                }
            }
            ForegroundSession::None => {}
        }
        Action::None
    }

    /// "Start Ingest": straight to project selection, unless a share is open
    fn start_clicked(&mut self, app_state: &AgentAppState) -> Action {
        if app_state.agent.foreground.share_id().is_none() {
            return Action::CreateSession;
        }
        if app_state.signed_in.is_none() {
            return Action::GoToLogin {
                destination: LoginDestination::StartSession,
            };
        }
        self.open_start_dialog();
        Action::None
    }

    pub(crate) fn open_start_dialog(&mut self) {
        self.start_dialog_open = true;
        self.focus_start_cancel = true;
    }

    /// Ask what happens to the open share before starting another one
    fn show_start_dialog(&mut self, ui: &mut egui::Ui, app_state: &AgentAppState) -> Action {
        let kind = app_state
            .signed_in
            .as_ref()
            .and_then(|signed_in| start_dialog_kind(&app_state.agent.foreground, signed_in));
        let Some(kind) = kind else {
            // signed out, or the share is gone in the meantime
            self.start_dialog_open = false;
            return Action::None;
        };
        let instrument = instrument_name(app_state.identity.as_ref());
        let (message, choices) = match &kind {
            StartDialog::OwnShare { previous } => (
                format!(
                    "Your ingest share for session {} in project {} is still running. Keep it \
                     next to the new one, or stop it once the new ingest has started?",
                    previous.session, previous.project
                ),
                vec![
                    DialogChoice {
                        label: "Keep it",
                        variant: ButtonVariant::Primary,
                        value: StartChoice::KeepOpen,
                    },
                    DialogChoice {
                        label: "Stop it after starting",
                        variant: ButtonVariant::DangerOutline,
                        value: StartChoice::ClosePrevious,
                    },
                ],
            ),
            StartDialog::OthersShare {
                project,
                session,
                user,
            } => (
                format!(
                    "{user} started the ingest share for session {session} in project {project}. \
                     Starting a new ingest makes it no longer available at {instrument}. Please \
                     coordinate with {user} first."
                ),
                vec![DialogChoice {
                    label: "Continue",
                    variant: ButtonVariant::Primary,
                    value: StartChoice::Continue,
                }],
            ),
        };
        let action = choice_dialog(
            ui.ctx(),
            ui.visuals(),
            START_DIALOG_ID,
            "Start another ingest?",
            &message,
            &choices,
            self.focus_start_cancel,
        );
        self.focus_start_cancel = false;

        match action {
            ChoiceDialogAction::None => Action::None,
            ChoiceDialogAction::Cancel => {
                self.start_dialog_open = false;
                Action::None
            }
            ChoiceDialogAction::Choose(StartChoice::KeepOpen | StartChoice::Continue) => {
                self.start_dialog_open = false;
                Action::CreateSession
            }
            ChoiceDialogAction::Choose(StartChoice::ClosePrevious) => {
                self.start_dialog_open = false;
                match kind {
                    StartDialog::OwnShare { previous } => {
                        Action::CreateSessionClosingPrevious { previous }
                    }
                    StartDialog::OthersShare { .. } => Action::CreateSession,
                }
            }
        }
    }

    pub fn poll(&mut self) -> Action {
        match self.close_task.poll() {
            Ok(task) => {
                if let Some(result) = task.take_ready() {
                    match result {
                        Ok(()) => self.close_succeeded(),
                        Err(error) => {
                            log::error!("close-session failed: {error}");
                            self.close_failed(TaskFailure::Stay {
                                message: "Could not stop the ingest share.".to_owned(),
                                retry: RetryAction::CloseSession,
                            })
                        }
                    }
                } else {
                    Action::None
                }
            }
            Err(error) => {
                log::error!("close-session task failed: {error}");
                self.close_failed(TaskFailure::Stay {
                    message: "Could not stop the ingest share.".to_owned(),
                    retry: RetryAction::CloseSession,
                })
            }
        }
    }

    pub fn is_busy(&self) -> bool {
        self.close_task.is_pending()
    }

    pub fn retry_close_session(&mut self, ctx: egui::Context) -> Action {
        self.close_task.reset();
        if let Some((signed_in, session)) = self.last_close.clone() {
            self.start_close(signed_in, session, ctx)
        } else {
            Action::None
        }
    }

    /// `hint` says what starting does to the running ingest share, if there is one
    fn show_create_section(
        &mut self,
        ui: &mut egui::Ui,
        resource: &str,
        hint: Option<&str>,
    ) -> bool {
        card_heading(ui, "Start ingest");
        card_description(
            ui,
            &format!(
                "Choose a project and session to create an ingest share for new data from \
                 {resource}."
            ),
        );
        if let Some(hint) = hint {
            card_description(ui, hint);
        }
        ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
        let response = ui.add(primary_button("Start Ingest", ui));
        if self.focus_on_entry && response.enabled() {
            response.request_focus();
            self.focus_on_entry = false;
        }
        response.clicked()
    }

    /// True if the user asked to release the previous user's ingest share (offered if `blocked`)
    fn show_mounting_session(
        &mut self,
        ui: &mut egui::Ui,
        session: &PendingSession,
        started_by: Option<&str>,
        status: &str,
        blocked: bool,
    ) -> bool {
        card_heading(ui, "Preparing ingest");
        session_detail(ui, "Project", &session.project);
        session_detail(ui, "Session", &session.session);
        if let Some(started_by) = started_by {
            session_detail(ui, "Started by", started_by);
        }
        ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
        ui.horizontal(|ui| {
            ui.spinner();
            card_description(ui, status);
        });
        if blocked {
            ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
            ui.add(danger_outline_button(
                "Disconnect previous user's ingest share",
                ui,
            ))
            .clicked()
        } else {
            false
        }
    }

    fn show_active_session(
        &mut self,
        ui: &mut egui::Ui,
        session: &ActiveSession,
        awaiting_close: bool,
        others_share: bool,
    ) -> bool {
        let is_closing = self.close_task.is_pending() || awaiting_close;
        card_heading(ui, "Foreground ingest");
        session_detail(ui, "Project", &session.project);
        session_detail(ui, "Session", &session.session);
        if let Some(started_by) = &session.created_by {
            session_detail(ui, "Started by", started_by);
        }
        match session.mount_path.as_deref() {
            Some(mount_path) => {
                match mount_path_detail(ui, mount_path) {
                    Some(MountPathAction::UseInPython) => {
                        self.paths_dialog_open = true;
                        self.focus_paths_close = true;
                    }
                    Some(MountPathAction::Open) => self.service.open_folder(mount_path),
                    None => {}
                }
                if self.paths_dialog_open {
                    self.show_paths_dialog(ui, mount_path);
                }
            }
            None => session_detail(
                ui,
                "Mount path on this computer",
                "Not available on this computer",
            ),
        }

        // the heading starts the next section; no divider needed
        ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);

        card_heading(ui, "Stop ingest");
        if !others_share {
            card_description(
                ui,
                "Stop the ingest share when you are finished transferring data.",
            );
        }
        ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
        let response = if is_closing {
            loading_button(
                ui,
                "Stopping…",
                egui::Vec2::ZERO,
                ButtonVariant::DangerOutline,
            )
        } else {
            // only the user who started it can stop it
            ui.add_enabled(!others_share, danger_outline_button("Stop Ingest", ui))
        };
        if self.focus_on_entry && response.enabled() {
            response.request_focus();
            self.focus_on_entry = false;
        }
        response.clicked()
    }

    /// The mount path and the notebook code, each with a copy button
    fn show_paths_dialog(&mut self, ui: &egui::Ui, mount_path: &str) {
        let ctx = ui.ctx();
        let snippet = python_snippet(self.service.agent_url());
        let response = egui::Modal::new(egui::Id::new(PATHS_DIALOG_ID))
            .frame(styles::card_frame(ui.visuals()))
            .show(ctx, |ui| {
                // in a narrow window, the dialog keeps the outer guides' distance to its edges;
                // the frame's padding and border come on top of the content width
                let frame = styles::spacing::card_padding_x::EGUI_POINTS
                    + styles::components::standard_border::width::EGUI_POINTS;
                let width = styles::components::wide_dialog::width::EGUI_POINTS
                    .min(ctx.content_rect().width() - 2.0 * (styles::outer_guide_inset() + frame));
                ui.set_width(width);
                ui.add(
                    Label::new(
                        RichText::new("Use in Python")
                            .font(styles::typography::section_heading::font_id()),
                    )
                    .wrap(),
                );
                ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
                copy_field(ui, "Mount path on this computer", mount_path);
                ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
                card_description(
                    ui,
                    "The session folder for new data changes whenever someone starts a new \
                     ingest. This Python code asks the modama agent on this computer for the \
                     current one, for example in a Jupyter notebook running here.",
                );
                ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
                copy_field(ui, "Python code", &snippet);
                ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close = ui.add(secondary_button("Close", ui));
                    if self.focus_paths_close {
                        close.request_focus();
                        self.focus_paths_close = false;
                    }
                    close.clicked()
                })
                .inner
            });
        if response.inner || response.should_close() {
            self.paths_dialog_open = false;
        }
    }

    fn show_close_confirmation(
        &mut self,
        ui: &mut egui::Ui,
        session: &ActiveSession,
        signed_in: &SignedIn,
    ) -> Action {
        let message = format!(
            "This will stop incoming data for session {} in project {}. Data already stored will \
             not be deleted.",
            session.session, session.project
        );
        let action = destructive_confirmation_dialog(
            ui.ctx(),
            ui.visuals(),
            CLOSE_CONFIRMATION_ID,
            "Stop ingest?",
            &message,
            "Stop Ingest",
            self.focus_confirmation_cancel,
        );
        self.focus_confirmation_cancel = false;

        match action {
            ConfirmationDialogAction::None => Action::None,
            ConfirmationDialogAction::Cancel => {
                self.close_confirmation_open = false;
                Action::None
            }
            ConfirmationDialogAction::Confirm => {
                self.close_confirmation_open = false;
                self.last_close = Some((signed_in.clone(), session.clone()));
                self.start_close(signed_in.clone(), session.clone(), ui.ctx().clone())
            }
        }
    }

    pub(crate) fn open_close_confirmation(&mut self) {
        self.close_mount = None;
        self.close_confirmation_open = true;
        self.focus_confirmation_cancel = true;
    }

    /// Ask before closing a share from the "Also mounted" list
    pub(crate) fn open_close_confirmation_for(&mut self, mount: ActiveSession) {
        self.open_close_confirmation();
        self.close_mount = Some(mount);
    }

    /// Without a foreground share, there's no foreground close to confirm; one for a share
    /// from the list stays open
    fn drop_foreground_close_confirmation(&mut self) {
        if self.close_mount.is_none() {
            self.close_confirmation_open = false;
        }
    }

    /// The compact list of the other shares mounted for this user, each with its own close
    fn show_other_mounts(&mut self, ui: &mut egui::Ui, app_state: &AgentAppState) -> Action {
        if app_state.agent.other_mounts.is_empty() {
            if self.close_mount.take().is_some() {
                self.close_confirmation_open = false;
            }
            return Action::None;
        }
        let pending_close = app_state.pending_close();
        let closing_any = self.close_task.is_pending() || pending_close.is_some();
        let mut clicked = None;
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            card_heading(ui, "Also mounted on this computer");
            for mount in &app_state.agent.other_mounts {
                ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
                ui.horizontal(|ui| {
                    let mut name = format!("{} / {}", mount.project, mount.session);
                    if let Some(user) = &mount.created_by {
                        name.push_str(" · ");
                        name.push_str(user);
                    }
                    ui.add(Label::new(
                        RichText::new(name).font(styles::typography::body::font_id()),
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if pending_close == Some(mount.share_id.as_str()) {
                            ui.add(Label::new(
                                RichText::new("Stopping…")
                                    .font(styles::typography::supporting_text::font_id())
                                    .color(
                                        styles::color::on_surface_dim::VALUE.resolve(ui.visuals()),
                                    ),
                            ));
                        } else if compact_close_button(ui, !closing_any).clicked() {
                            clicked = Some(mount.clone());
                        }
                    });
                });
                if let Some(mount_path) = &mount.mount_path {
                    ui.add(
                        Label::new(
                            RichText::new(mount_path)
                                .font(styles::typography::mono_medium::font_id())
                                .color(
                                    styles::color::on_surface_variant::VALUE.resolve(ui.visuals()),
                                ),
                        )
                        .wrap(),
                    );
                }
            }
        });

        if let Some(mount) = clicked {
            if app_state.signed_in.is_none() {
                return Action::GoToLogin {
                    destination: LoginDestination::ConfirmClose {
                        share_id: mount.share_id,
                    },
                };
            }
            self.open_close_confirmation_for(mount);
        }

        if self.close_confirmation_open
            && let Some(mount) = self.close_mount.clone()
        {
            let Some(signed_in) = &app_state.signed_in else {
                self.close_confirmation_open = false;
                self.close_mount = None;
                return Action::None;
            };
            let action = self.show_close_confirmation(ui, &mount, signed_in);
            if !self.close_confirmation_open {
                self.close_mount = None;
            }
            return action;
        }
        Action::None
    }

    #[cfg(test)]
    pub(crate) fn close_confirmation_is_open(&self) -> bool {
        self.close_confirmation_open
    }

    fn start_close(
        &mut self,
        signed_in: SignedIn,
        session: ActiveSession,
        ctx: egui::Context,
    ) -> Action {
        let operation_id = OperationId::next();
        self.close_operation_id = Some(operation_id);
        let share_id = session.share_id.clone();
        let service = Arc::clone(&self.service);
        self.close_task
            .spawn(move || service.close_session(&signed_in, &session), ctx);
        Action::CloseRequested {
            operation_id,
            share_id,
        }
    }

    fn close_succeeded(&mut self) -> Action {
        let Some(operation_id) = self.close_operation_id.take() else {
            log::error!("close task completed without an operation ID");
            return Action::None;
        };
        Action::CloseSessionSuccess { operation_id }
    }

    fn close_failed(&mut self, failure: TaskFailure) -> Action {
        let Some(operation_id) = self.close_operation_id.take() else {
            log::error!("close task failed without an operation ID");
            return Action::None;
        };
        Action::CloseFailed {
            operation_id,
            failure,
        }
    }
}

/// Two halves of a card side by side, with a vertical bar in the gap between them; one above
/// the other, with a horizontal bar, when the card is too narrow
struct Columns {
    first: egui::Ui,
    second: Option<egui::Ui>,
    column_width: f32,
    side_by_side: bool,
}

impl Columns {
    /// Below this width, each half would be too narrow for its texts
    const MIN_SIDE_BY_SIDE_WIDTH: f32 = 560.0;

    fn new(ui: &mut egui::Ui) -> Self {
        let width = ui.available_width();
        let side_by_side = width >= Self::MIN_SIDE_BY_SIDE_WIDTH;
        let column_width = if side_by_side {
            (width - Self::gap()) / 2.0
        } else {
            width
        };
        let first = Self::column(ui, ui.cursor().min, column_width);
        Self {
            first,
            second: None,
            column_width,
            side_by_side,
        }
    }

    /// The gap between the halves: the card's padding on each side of the bar
    fn gap() -> f32 {
        2.0 * styles::components::content_card_padding_x::padding::EGUI_POINTS
    }

    fn column(ui: &mut egui::Ui, top_left: egui::Pos2, width: f32) -> egui::Ui {
        ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(
                    top_left,
                    egui::vec2(width, ui.available_height()),
                ))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        )
    }

    fn first(&mut self) -> &mut egui::Ui {
        &mut self.first
    }

    /// Call once the first half is complete
    fn second(&mut self, ui: &mut egui::Ui) -> &mut egui::Ui {
        let first = self.first.min_rect();
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        let top_left = if self.side_by_side {
            let left = first.left() + self.column_width + Self::gap();
            ui.painter()
                .vline(left - Self::gap() / 2.0, first.y_range(), stroke);
            egui::pos2(left, first.top())
        } else {
            let gap = styles::spacing::form_field_gap::EGUI_POINTS;
            let bar_y = first.bottom() + gap;
            ui.painter().hline(first.x_range(), bar_y, stroke);
            egui::pos2(first.left(), bar_y + gap)
        };
        self.second
            .insert(Self::column(ui, top_left, self.column_width))
    }

    fn finish(self, ui: &mut egui::Ui) {
        let mut rect = self.first.min_rect();
        if let Some(second) = &self.second {
            rect = rect.union(second.min_rect());
        }
        ui.allocate_rect(rect, egui::Sense::hover());
    }
}

/// The agents connected at this agent's resource, below the start action; the list scrolls on
/// its own once it is longer than a few rows
fn show_connected_agents(ui: &mut egui::Ui, connected_agents: Option<&ConnectedAgents>) {
    /// The list shows this many rows before it scrolls
    const VISIBLE_ROWS: f32 = 4.0;
    let row_height = styles::components::connected_agent_row::height::EGUI_POINTS;

    // the heading starts the next section; no divider needed
    ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);

    let Some(agents) = connected_agents else {
        card_heading(ui, "Connected agents");
        card_description(ui, "Not connected to the server yet.");
        return;
    };
    card_heading(ui, &format!("Connected agents at {}", agents.resource));
    ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
    egui::Frame::new()
        .stroke(egui::Stroke::new(
            styles::components::standard_border::width::EGUI_POINTS,
            styles::color::outline::VALUE.resolve(ui.visuals()),
        ))
        .corner_radius(styles::radius(styles::rounded::lg::EGUI_POINTS))
        .inner_margin(egui::Margin::symmetric(
            styles::spacing::field_label_gap::EGUI_POINTS as i8 * 2,
            styles::spacing::field_label_gap::EGUI_POINTS as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            styles::scrollbar_scope(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("connected-agents")
                    .max_height(VISIBLE_ROWS * row_height)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // each row in its own ID scope: rows with and without a lock would
                        // otherwise hand out the same automatic IDs
                        ui.push_id("own", |ui| {
                            connected_agent_row(ui, &agents.own, true, row_height);
                        });
                        for (index, agent) in agents.others.iter().enumerate() {
                            ui.push_id(index, |ui| {
                                connected_agent_row(ui, agent, false, row_height);
                            });
                        }
                    });
            });
        });
    if agents.others.is_empty() {
        ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
        card_description(ui, "No other agents are connected.");
    }
}

/// The hover text of a connected agent's lock or (i): whether its identity is verified, whose
/// token an anonymous agent uses, and its connection to smbdrop, as smbdrop logs it
fn identity_hover_text(agent: &ConnectedAgent) -> String {
    let mut lines = vec![if agent.verified {
        "Identity verified".to_owned()
    } else {
        "Identity not verified".to_owned()
    }];
    let connection = &agent.connection;
    if let Some(lender) = &connection.lender {
        lines.push(format!("Token from {lender}"));
    }
    lines.push(format!(
        "Connection {} from {}",
        connection.id, connection.peer
    ));
    lines.join("\n")
}

/// How a connected agent's mount status shows as the dot in front of its name
#[derive(Debug, PartialEq)]
struct StatusDot {
    color: styles::ThemeColor,
    /// A ring for an unknown status
    filled: bool,
    /// The dot's hover text; the HTML pages use the same words
    label: &'static str,
}

fn status_dot(status: Option<AgentMountStatus>) -> StatusDot {
    let (color, filled, label) = match status {
        Some(AgentMountStatus::Idle) => (styles::color::tertiary::VALUE, true, "idle"),
        Some(AgentMountStatus::Reconciling) => (styles::color::primary::VALUE, true, "reconciling"),
        Some(AgentMountStatus::LeftoverWork) => (
            styles::color::secondary::VALUE,
            true,
            "reconciled, with work left over",
        ),
        Some(AgentMountStatus::NoHelper) => (
            styles::color::outline_variant::VALUE,
            true,
            "no session helper",
        ),
        Some(AgentMountStatus::Unknown) | None => (
            styles::color::outline_variant::VALUE,
            false,
            "status unknown",
        ),
    };
    StatusDot {
        color,
        filled,
        label,
    }
}

/// `● name 🔒 … this agent`: the dot shows the mount status, the lock that the identity is
/// verified, an (i) in its place that it isn't
fn connected_agent_row(ui: &mut egui::Ui, agent: &ConnectedAgent, own: bool, height: f32) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, height));
            let dot_size = styles::components::connected_agent_dot::size::EGUI_POINTS;
            let (dot, response) =
                ui.allocate_exact_size(egui::vec2(dot_size, dot_size), egui::Sense::hover());
            let StatusDot {
                color,
                filled,
                label,
            } = status_dot(agent.status);
            let color = color.resolve(ui.visuals());
            if filled {
                ui.painter()
                    .circle_filled(dot.center(), dot_size / 2.0, color);
            } else {
                let width = styles::components::standard_border::width::EGUI_POINTS;
                ui.painter().circle_stroke(
                    dot.center(),
                    (dot_size - width) / 2.0,
                    egui::Stroke::new(width, color),
                );
            }
            response.on_hover_text(label);
            ui.add(
                Label::new(
                    RichText::new(&agent.name).font(styles::typography::mono_medium::font_id()),
                )
                .truncate(),
            );
            // the lock for a verified identity, the (i) for an anonymous agent; either way, the
            // connection's details on hover
            let icon = if agent.verified {
                egui_phosphor::regular::LOCK_SIMPLE
            } else {
                egui_phosphor::regular::INFO
            };
            ui.add(Label::new(
                // phosphor's glyphs are in the proportional family
                RichText::new(icon)
                    .font(egui::FontId::proportional(
                        styles::typography::supporting_text::font_id().size,
                    ))
                    .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
            ))
            .on_hover_text(identity_hover_text(agent));
            if own {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(Label::new(
                        RichText::new("this agent")
                            .font(styles::typography::supporting_text::font_id())
                            .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
                    ));
                });
            }
        },
    );
}

fn card_heading(ui: &mut egui::Ui, text: &str) {
    ui.add(Label::new(
        RichText::new(text).font(styles::typography::section_heading::font_id()),
    ));
}

fn card_description(ui: &mut egui::Ui, text: &str) {
    // wraps also next to a spinner, in a horizontal layout
    ui.add(
        Label::new(
            RichText::new(text)
                .font(styles::typography::body::font_id())
                .color(styles::color::on_surface_variant::VALUE.resolve(ui.visuals())),
        )
        .wrap(),
    );
}

/// A small text button for a secondary action next to an item, like closing one of several
/// shares; deliberately less prominent than the cards' buttons
fn compact_close_button(ui: &mut egui::Ui, enabled: bool) -> egui::Response {
    let button = egui::Button::new(
        RichText::new("Stop")
            .font(styles::typography::supporting_text::font_id())
            .color(styles::color::error::VALUE.resolve(ui.visuals())),
    )
    .frame(false);
    let response = ui.add_enabled(enabled, button);
    styles::paint_focus_ring(ui, &response);
    response
}

/// What the user asked to do with the mount path
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MountPathAction {
    Open,
    UseInPython,
}

/// How long "Copied" replaces "Copy" after copying
const COPIED_FEEDBACK_SECONDS: f64 = 2.0;

/// The toolbar buttons' padding; the strip's margins subtract it, so that the icons line up
/// with the path above
const TOOLBAR_BUTTON_PADDING: egui::Vec2 = egui::vec2(
    styles::components::path_box_toolbar_button_padding_x::padding::EGUI_POINTS,
    styles::components::path_box_toolbar_button_padding_y::padding::EGUI_POINTS,
);

/// The mount path in a box, on one line until clicked, with a toolbar below a divider to
/// copy it, open it in the file manager and use it in Python. Copying happens here.
fn mount_path_detail(ui: &mut egui::Ui, mount_path: &str) -> Option<MountPathAction> {
    ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
    let padding = styles::spacing::input_padding_x::EGUI_POINTS;
    let border = egui::Stroke::new(
        styles::components::standard_border::width::EGUI_POINTS,
        styles::color::outline::VALUE.resolve(ui.visuals()),
    );
    let fill = styles::color::surface_variant::VALUE.resolve(ui.visuals());
    // dividers span the box from border to border
    let divider = |ui: &mut egui::Ui| {
        let y = ui.cursor().top();
        ui.painter().hline(ui.max_rect().x_range(), y, border);
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(border)
        .corner_radius(styles::radius(styles::rounded::lg::EGUI_POINTS))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(
                    padding as i8,
                    styles::components::path_box_header_padding_y::padding::EGUI_POINTS as i8,
                ))
                .show(ui, |ui| detail_label(ui, "Mount path on this computer"));
            divider(ui);
            egui::Frame::new()
                .inner_margin(egui::Margin::same(padding as i8))
                .show(ui, |ui| {
                    path_line(
                        ui,
                        "foreground-mount-path",
                        mount_path,
                        styles::typography::mono_medium::font_id(),
                        fill,
                    );
                });
            divider(ui);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(
                    (padding - TOOLBAR_BUTTON_PADDING.x) as i8,
                    styles::components::path_box_toolbar_padding_y::padding::EGUI_POINTS as i8,
                ))
                .show(ui, |ui| {
                    ui.horizontal(|ui| mount_path_actions(ui, mount_path)).inner
                })
                .inner
        })
        .inner
}

/// Copy, open and use-in-Python as small buttons
fn mount_path_actions(ui: &mut egui::Ui, mount_path: &str) -> Option<MountPathAction> {
    let copy_id = ui.id().with("copy-mount-path");
    let now = ui.input(|input| input.time);
    let copied_at: Option<f64> = ui.data(|data| data.get_temp(copy_id));
    let copied = copied_at.is_some_and(|at| now - at < COPIED_FEEDBACK_SECONDS);
    let (copy_icon, copy_label) = if copied {
        (egui_phosphor::regular::CHECK_CIRCLE, "Copied")
    } else {
        (egui_phosphor::regular::COPY, "Copy")
    };

    let mut action = None;
    if small_action_button(ui, copy_icon, copy_label).clicked() {
        ui.ctx().copy_text(mount_path.to_owned());
        ui.data_mut(|data| data.insert_temp(copy_id, now));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(COPIED_FEEDBACK_SECONDS));
    }
    if small_action_button(ui, egui_phosphor::regular::FOLDER_OPEN, "Open folder").clicked() {
        action = Some(MountPathAction::Open);
    }
    if small_action_button(ui, egui_phosphor::regular::CODE, "Use in Python").clicked() {
        action = Some(MountPathAction::UseInPython);
    }
    action
}

/// An icon and a short label in supporting text, quieter than the cards' buttons
fn small_action_button(ui: &mut egui::Ui, icon: &str, label: &str) -> egui::Response {
    let font = styles::typography::supporting_text::font_id();
    let color = styles::color::on_surface_variant::VALUE.resolve(ui.visuals());
    let mut text = egui::text::LayoutJob::default();
    text.append(icon, 0.0, egui::TextFormat::simple(icon_font(&font), color));
    text.append(
        label,
        styles::spacing::field_label_gap::EGUI_POINTS,
        egui::TextFormat::simple(font, color),
    );
    let button =
        egui::Button::new(text).corner_radius(styles::radius(styles::rounded::lg::EGUI_POINTS));
    let surface = styles::color::surface::VALUE.resolve(ui.visuals());
    let outline = egui::Stroke::new(
        styles::components::standard_border::width::EGUI_POINTS,
        styles::color::outline::VALUE.resolve(ui.visuals()),
    );
    // only text, white with a border under the pointer
    let (idle_fill, idle_stroke, hover_fill) =
        (egui::Color32::TRANSPARENT, egui::Stroke::NONE, surface);
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding = TOOLBAR_BUTTON_PADDING;
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.weak_bg_fill = idle_fill;
            widgets.inactive.bg_stroke = idle_stroke;
            for state in [&mut widgets.hovered, &mut widgets.active] {
                state.weak_bg_fill = hover_fill;
                state.bg_stroke = outline;
                state.expansion = 0.0;
            }
            ui.add(button)
        })
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    styles::paint_focus_ring(ui, &response);
    response
}

fn session_detail(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
    detail_label(ui, label);
    ui.add_space(styles::spacing::detail_label_gap::EGUI_POINTS - ui.spacing().item_spacing.y);
    // long values (mount paths) wrap; egui breaks them at a dash or other punctuation before
    // breaking anywhere, and copying the label still gives the unbroken value
    ui.add(
        Label::new(RichText::new(value).font(styles::typography::mono_medium::font_id())).wrap(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AgentIdentity;

    #[test]
    fn hover_text_says_how_an_agent_is_connected() {
        use modama_common::{internal_api_types::ConnectionDetails, messages::ConnectionId};

        let mut agent = ConnectedAgent {
            name: "neighbour-pc".to_owned(),
            verified: false,
            status: None,
            connection: ConnectionDetails {
                id: ConnectionId(17),
                peer: "192.0.2.1:53810".parse().unwrap(),
                lender: Some("dev".to_owned()),
            },
        };
        assert_eq!(
            identity_hover_text(&agent),
            "Identity not verified\nToken from dev\nConnection #17 from 192.0.2.1:53810"
        );
        agent.verified = true;
        agent.connection.lender = None;
        assert_eq!(
            identity_hover_text(&agent),
            "Identity verified\nConnection #17 from 192.0.2.1:53810"
        );
    }

    #[test]
    fn every_mount_status_has_its_own_dot() {
        let statuses = [
            Some(AgentMountStatus::Idle),
            Some(AgentMountStatus::Reconciling),
            Some(AgentMountStatus::LeftoverWork),
            Some(AgentMountStatus::NoHelper),
            None,
        ];
        let dots: Vec<StatusDot> = statuses.into_iter().map(status_dot).collect();
        for (i, a) in dots.iter().enumerate() {
            for b in &dots[i + 1..] {
                assert!((a.color, a.filled) != (b.color, b.filled), "{a:?} vs {b:?}");
                assert_ne!(a.label, b.label);
            }
        }
        // only the unknown status is a ring
        assert_eq!(dots.iter().filter(|dot| !dot.filled).count(), 1, "{dots:?}");
        assert_eq!(
            status_dot(Some(AgentMountStatus::Unknown)),
            status_dot(None)
        );
    }

    fn alice() -> SignedIn {
        SignedIn {
            username: "alice".to_owned(),
        }
    }

    fn open_share(created_by: Option<&str>) -> ActiveSession {
        ActiveSession {
            share_id: "open".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            mount_path: Some("/mnt/session".to_owned()),
            created_by: created_by.map(str::to_owned),
        }
    }

    #[test]
    fn own_and_unknown_shares_can_be_kept_or_closed_others_are_detached() {
        for created_by in [Some("alice"), None] {
            let previous = open_share(created_by);
            assert_eq!(
                start_dialog_kind(&ForegroundSession::Mounted(previous.clone()), &alice()),
                Some(StartDialog::OwnShare { previous })
            );
        }
        assert_eq!(
            start_dialog_kind(
                &ForegroundSession::Mounted(open_share(Some("bob"))),
                &alice()
            ),
            Some(StartDialog::OthersShare {
                project: "project".to_owned(),
                session: "session".to_owned(),
                user: "bob".to_owned(),
            })
        );
        assert_eq!(start_dialog_kind(&ForegroundSession::None, &alice()), None);
    }

    #[test]
    fn a_share_still_mounting_counts_as_open() {
        let pending = PendingSession {
            share_id: "open".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            created_by: Some("alice".to_owned()),
        };
        let mounting = ForegroundSession::Mounting(pending, "Waiting".to_owned());
        assert_eq!(
            start_dialog_kind(&mounting, &alice()),
            Some(StartDialog::OwnShare {
                previous: ActiveSession {
                    mount_path: None,
                    ..open_share(Some("alice"))
                }
            })
        );
    }

    #[test]
    fn start_goes_straight_to_projects_unless_a_share_is_open() {
        let mut page = PageOverview::new();
        let mut app_state = AgentAppState::new();
        assert_eq!(page.start_clicked(&app_state), Action::CreateSession);

        app_state.agent.foreground = ForegroundSession::Mounted(open_share(Some("alice")));
        assert_eq!(
            page.start_clicked(&app_state),
            Action::GoToLogin {
                destination: LoginDestination::StartSession
            }
        );
        assert!(!page.start_dialog_is_open());

        app_state.signed_in = Some(alice());
        assert_eq!(page.start_clicked(&app_state), Action::None);
        assert!(page.start_dialog_is_open());
    }

    #[test]
    fn the_start_hint_says_what_happens_to_the_open_share() {
        let mut app_state = AgentAppState::new();
        app_state.agent.foreground = ForegroundSession::Mounted(open_share(Some("bob")));
        // signed out, it depends on who signs in:
        assert_eq!(start_hint(&app_state), None);

        app_state.signed_in = Some(alice());
        app_state.identity = Some(AgentIdentity::AgentId {
            agent_name: "DLD".to_owned(),
            resource_name: "Holo".to_owned(),
        });
        assert_eq!(
            start_hint(&app_state).as_deref(),
            Some(
                "bob's ingest share for session session will no longer be available at Holo. \
                 Please coordinate with bob first."
            )
        );

        app_state.agent.foreground = ForegroundSession::Mounted(open_share(Some("alice")));
        assert_eq!(
            start_hint(&app_state).as_deref(),
            Some(
                "Your ingest share for session session keeps running, unless you choose to stop it."
            )
        );
        app_state.agent.foreground = ForegroundSession::Mounted(open_share(None));
        assert_eq!(
            start_hint(&app_state).as_deref(),
            Some(
                "The ingest share for session session keeps running, unless you choose to stop it."
            )
        );

        app_state.agent.foreground = ForegroundSession::None;
        assert_eq!(start_hint(&app_state), None);
    }

    #[test]
    fn test_overview_poll_returns_none() {
        let mut page = PageOverview::new();
        assert_eq!(page.poll(), Action::None);
    }
}
