use eframe::egui::{self, Label, RichText};
use std::sync::Arc;

use crate::{
    actions::{Action, LoginDestination, RetryAction, TaskFailure},
    async_state::AsyncState,
    components::{
        buttons::{ButtonVariant, danger_outline_button, loading_button, primary_button},
        dialogs::{
            ChoiceDialogAction, ConfirmationDialogAction, DialogChoice, choice_dialog,
            destructive_confirmation_dialog,
        },
        page_heading,
    },
    service::{AgentService, ServiceError, StubAgentService},
    state::{
        ActiveSession, AgentAppState, ForegroundSession, OperationId, PendingSession, SignedIn,
        instrument_name,
    },
    styles,
};

const CLOSE_CONFIRMATION_ID: &str = "close-session-confirmation";
const START_DIALOG_ID: &str = "start-session-while-open";

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
    /// Someone else's: starting detaches it from this instrument
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
    match &previous.created_by {
        Some(user) if *user != signed_in.username => Some(StartDialog::OthersShare {
            project: previous.project.clone(),
            session: previous.session.clone(),
            user: user.clone(),
        }),
        _ => Some(StartDialog::OwnShare { previous }),
    }
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
    pub(crate) fn start_dialog_is_open(&self) -> bool {
        self.start_dialog_open
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
        app_state: &AgentAppState,
    ) -> Action {
        let pending_start = app_state.pending_start();
        page_heading(ui, "Overview");
        ui.add_space(styles::content_card_top_margin_adjustment());

        // the user asked to close their previous share once the new one has started
        if let Some((signed_in, previous)) = self.close_previous.take() {
            self.last_close = Some((signed_in.clone(), previous.clone()));
            return self.start_close(signed_in, previous, ui.ctx().clone());
        }

        if let Some(session) = pending_start {
            self.close_confirmation_open = false;
            // a share being started has no creator on record yet; it's the user
            let started_by = session.created_by.as_deref().or(app_state
                .signed_in
                .as_ref()
                .map(|signed_in| signed_in.username.as_str()));
            self.show_mounting_session_card(ui, session, started_by, "Waiting for agent…", false);
        } else {
            let action = self.show_foreground_card(ui, app_state);
            if action != Action::None {
                return action;
            }
            if self.show_create_card(ui) {
                return self.start_clicked(app_state);
            }
        }

        if self.start_dialog_open {
            return self.show_start_dialog(ui, app_state);
        }

        Action::None
    }

    /// The card of the foreground share, if there is one
    fn show_foreground_card(&mut self, ui: &mut egui::Ui, app_state: &AgentAppState) -> Action {
        let pending_close = app_state.pending_close();
        let signed_in = &app_state.signed_in;
        match &app_state.foreground {
            ForegroundSession::Mounted(session) => {
                let close_clicked = self.show_active_session_card(
                    ui,
                    session,
                    pending_close == Some(&session.share_id),
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
                if self.close_confirmation_open {
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
                self.close_confirmation_open = false;
                let started_by = session.created_by.as_deref();
                if self.show_mounting_session_card(
                    ui,
                    session,
                    started_by,
                    message,
                    app_state.hand_off_blocked,
                ) {
                    return Action::ForceRelease;
                }
            }
            ForegroundSession::None => {
                self.close_confirmation_open = false;
            }
        }
        Action::None
    }

    /// "Start Session": straight to project selection, unless a share is open
    fn start_clicked(&mut self, app_state: &AgentAppState) -> Action {
        if app_state.foreground.share_id().is_none() {
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
            .and_then(|signed_in| start_dialog_kind(&app_state.foreground, signed_in));
        let Some(kind) = kind else {
            // signed out, or the share is gone in the meantime
            self.start_dialog_open = false;
            return Action::None;
        };
        let instrument = instrument_name(app_state.identity.as_ref());
        let (message, choices) = match &kind {
            StartDialog::OwnShare { previous } => (
                format!(
                    "Session {} in project {} is open. Keep it open next to the new one, or \
                     close it once the new session has started?",
                    previous.session, previous.project
                ),
                vec![
                    DialogChoice {
                        label: "Keep it open",
                        variant: ButtonVariant::Primary,
                        value: StartChoice::KeepOpen,
                    },
                    DialogChoice {
                        label: "Close it after starting",
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
                    "Session {session} in project {project} was started by {user}. Starting a \
                     new session detaches it from {instrument}; it stays open for {user}."
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
            "Start another ingest session?",
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
                                message: "Could not close the ingest session.".to_owned(),
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
                    message: "Could not close the ingest session.".to_owned(),
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

    fn show_create_card(&mut self, ui: &mut egui::Ui) -> bool {
        styles::content_card_frame(ui.visuals())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_heading(ui, "Start an ingest session");
                card_description(
                    ui,
                    "Choose a project and session to open an ingest share for incoming data on \
                     this agent.",
                );
                ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
                let response = ui.add(primary_button("Start Session", ui));
                if self.focus_on_entry && response.enabled() {
                    response.request_focus();
                    self.focus_on_entry = false;
                }
                response.clicked()
            })
            .inner
    }

    /// True if the user asked to release the previous user's drive (offered if `blocked`)
    fn show_mounting_session_card(
        &mut self,
        ui: &mut egui::Ui,
        session: &PendingSession,
        started_by: Option<&str>,
        status: &str,
        blocked: bool,
    ) -> bool {
        styles::content_card_frame(ui.visuals())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_heading(ui, "Preparing ingest session");
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
                        "Disconnect previous user's drive",
                        ui,
                    ))
                    .clicked()
                } else {
                    false
                }
            })
            .inner
    }

    fn show_active_session_card(
        &mut self,
        ui: &mut egui::Ui,
        session: &ActiveSession,
        awaiting_close: bool,
    ) -> bool {
        let is_closing = self.close_task.is_pending() || awaiting_close;
        styles::content_card_frame(ui.visuals())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                card_heading(ui, "Active ingest session");
                session_detail(ui, "Project", &session.project);
                session_detail(ui, "Session", &session.session);
                if let Some(started_by) = &session.created_by {
                    session_detail(ui, "Started by", started_by);
                }
                session_detail(
                    ui,
                    "Mount path on this agent",
                    session
                        .mount_path
                        .as_deref()
                        .unwrap_or("Not available on this agent"),
                );

                ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
                ui.separator();
                ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);

                card_heading(ui, "Close the ingest session");
                card_description(
                    ui,
                    "Stop the ingest share when you are finished transferring data. Existing \
                     project and session data will remain available.",
                );
                ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
                let response = if is_closing {
                    loading_button(
                        ui,
                        "Closing…",
                        egui::Vec2::ZERO,
                        ButtonVariant::DangerOutline,
                    )
                } else {
                    ui.add(danger_outline_button("Close Session", ui))
                };
                if self.focus_on_entry && response.enabled() {
                    response.request_focus();
                    self.focus_on_entry = false;
                }
                response.clicked()
            })
            .inner
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
            "Close ingest session?",
            &message,
            "Close Session",
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
        self.close_confirmation_open = true;
        self.focus_confirmation_cancel = true;
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

fn card_heading(ui: &mut egui::Ui, text: &str) {
    ui.add(Label::new(
        RichText::new(text).font(styles::typography::section_heading::font_id()),
    ));
}

fn card_description(ui: &mut egui::Ui, text: &str) {
    ui.add(Label::new(
        RichText::new(text)
            .font(styles::typography::body::font_id())
            .color(styles::color::on_surface_variant::VALUE.resolve(ui.visuals())),
    ));
}

fn session_detail(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
    ui.add(Label::new(
        RichText::new(label)
            .font(styles::typography::supporting_text::font_id())
            .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
    ));
    ui.add(Label::new(
        RichText::new(value).font(styles::typography::mono_medium::font_id()),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

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

        app_state.foreground = ForegroundSession::Mounted(open_share(Some("alice")));
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
    fn test_overview_poll_returns_none() {
        let mut page = PageOverview::new();
        assert_eq!(page.poll(), Action::None);
    }
}
