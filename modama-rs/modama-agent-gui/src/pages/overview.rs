use eframe::egui::{self, Label, RichText};
use std::sync::Arc;

use crate::{
    actions::{Action, LoginDestination, RetryAction, TaskFailure},
    async_state::AsyncState,
    components::{
        buttons::{ButtonVariant, danger_outline_button, loading_button, primary_button},
        dialogs::{ConfirmationDialogAction, destructive_confirmation_dialog},
        page_heading,
    },
    service::{AgentService, ServiceError, StubAgentService},
    state::{
        ActiveSession, AgentAppState, ForegroundSession, OperationId, PendingSession, SignedIn,
    },
    styles,
};

const CLOSE_CONFIRMATION_ID: &str = "close-session-confirmation";

pub struct PageOverview {
    pub close_task: AsyncState<Result<(), ServiceError>>,
    close_confirmation_open: bool,
    focus_confirmation_cancel: bool,
    focus_on_entry: bool,
    service: Arc<dyn AgentService>,
    last_close: Option<(SignedIn, ActiveSession)>,
    close_operation_id: Option<OperationId>,
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
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
        app_state: &AgentAppState,
    ) -> Action {
        let foreground = &app_state.foreground;
        let pending_start = app_state.pending_start();
        let pending_close = app_state.pending_close();
        let signed_in = &app_state.signed_in;
        page_heading(ui, "Overview");
        ui.add_space(styles::content_card_top_margin_adjustment());

        match foreground {
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
                if self.show_mounting_session_card(ui, session, message, app_state.hand_off_blocked)
                {
                    return Action::ForceRelease;
                }
            }
            ForegroundSession::None => {
                self.close_confirmation_open = false;
                if let Some(session) = pending_start {
                    self.show_mounting_session_card(ui, session, "Waiting for agent…", false);
                } else if self.show_create_card(ui) {
                    return Action::CreateSession;
                }
            }
        }

        Action::None
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
        status: &str,
        blocked: bool,
    ) -> bool {
        styles::content_card_frame(ui.visuals())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_heading(ui, "Preparing ingest session");
                session_detail(ui, "Project", &session.project);
                session_detail(ui, "Session", &session.session);
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

    #[test]
    fn test_overview_poll_returns_none() {
        let mut page = PageOverview::new();
        assert_eq!(page.poll(), Action::None);
    }
}
