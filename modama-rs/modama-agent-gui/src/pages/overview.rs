use eframe::egui::{self, Label, RichText};
use std::sync::Arc;

use crate::{
    actions::{Action, RetryAction, TaskFailure},
    async_state::AsyncState,
    components::{
        buttons::{danger_outline_button, loading_button, primary_button, ButtonVariant},
        dialogs::{destructive_confirmation_dialog, ConfirmationDialogAction},
        page_heading,
    },
    service::{AgentService, ServiceError, StubAgentService},
    state::{ActiveSession, Credentials, ForegroundSession, PendingSession},
    styles,
};

const CLOSE_CONFIRMATION_ID: &str = "close-session-confirmation";

pub struct PageOverview {
    pub close_task: AsyncState<Result<(), ServiceError>>,
    close_confirmation_open: bool,
    focus_confirmation_cancel: bool,
    focus_on_entry: bool,
    service: Arc<dyn AgentService>,
    last_close: Option<(Credentials, ActiveSession)>,
    login_required: bool, // FIXME: sucks to have this as state, why cant the close confirmation
                          // thing return the action instead?
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
            login_required: false,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
        foreground: &ForegroundSession,
        pending_start: &Option<PendingSession>,
        pending_close: &Option<String>,
        credentials: &Option<Credentials>,
    ) -> Action {
        page_heading(ui, "Overview");
        ui.add_space(styles::content_card_top_margin_adjustment());

        match foreground {
            ForegroundSession::Mounted(session) => {
                self.show_active_session_card(
                    ui,
                    session,
                    pending_close.as_deref() == Some(&session.share_id),
                );
                if self.close_confirmation_open {
                    self.show_close_confirmation(ui, session, credentials);
                }
            }
            ForegroundSession::Mounting(session) => {
                self.close_confirmation_open = false;
                self.show_mounting_session_card(ui, session, "Mounting ingest share…");
            }
            ForegroundSession::Unmounting(session) => {
                self.close_confirmation_open = false;
                self.show_unmounting_session_card(ui, session);
            }
            ForegroundSession::None => {
                self.close_confirmation_open = false;
                if let Some(session) = pending_start {
                    self.show_mounting_session_card(ui, session, "Waiting for agent…");
                } else if self.show_create_card(ui) {
                    return Action::CreateSession;
                }
            }
        }

        Action::None
    }

    pub fn poll(&mut self) -> Action {
        if std::mem::take(&mut self.login_required) {
            return Action::GoToLogin;
        }
        match self.close_task.poll() {
            Ok(task) => {
                if let Some(result) = task.take_ready() {
                    match result {
                        Ok(()) => self
                            .last_close
                            .as_ref()
                            .map(|(_, session)| Action::CloseSessionSuccess {
                                share_id: session.share_id.clone(),
                            })
                            .unwrap_or(Action::None),
                        Err(error) => {
                            log::error!("close-session failed: {error}");
                            Action::TaskFailed(TaskFailure::Stay {
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
                Action::TaskFailed(TaskFailure::Stay {
                    message: "Could not close the ingest session.".to_owned(),
                    retry: RetryAction::CloseSession,
                })
            }
        }
    }

    pub fn is_busy(&self) -> bool {
        self.close_task.is_pending()
    }

    pub fn retry_close_session(&mut self, ctx: egui::Context) {
        self.close_task.reset();
        if let Some((credentials, session)) = self.last_close.clone() {
            self.spawn_close(credentials, session, ctx);
        }
    }

    fn show_create_card(&mut self, ui: &mut egui::Ui) -> bool {
        let mut clicked = false;
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            card_heading(ui, "Start an ingest session");
            card_description(
                ui,
                "Choose a project and session to open an ingest share for incoming data on this \
                 agent.",
            );
            ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
            let response = ui.add(primary_button("Start Session", ui));
            if self.focus_on_entry && response.enabled() {
                response.request_focus();
                self.focus_on_entry = false;
            }
            clicked = response.clicked();
        });
        clicked
    }

    fn show_mounting_session_card(
        &mut self,
        ui: &mut egui::Ui,
        session: &PendingSession,
        status: &str,
    ) {
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            card_heading(ui, "Preparing ingest session");
            session_detail(ui, "Project", &session.project);
            session_detail(ui, "Session", &session.session);
            ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
            ui.horizontal(|ui| {
                ui.spinner();
                card_description(ui, status);
            });
        });
    }

    fn show_active_session_card(
        &mut self,
        ui: &mut egui::Ui,
        session: &ActiveSession,
        awaiting_close: bool,
    ) {
        let is_closing = self.close_task.is_pending() || awaiting_close;
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
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
                "Stop the ingest share when you are finished transferring data. Existing project \
                 and session data will remain available.",
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
            if response.clicked() {
                self.close_confirmation_open = true;
                self.focus_confirmation_cancel = true;
            }
        });
    }

    fn show_unmounting_session_card(&mut self, ui: &mut egui::Ui, session: &ActiveSession) {
        styles::content_card_frame(ui.visuals()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            card_heading(ui, "Closing ingest session");
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
            ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
            ui.horizontal(|ui| {
                ui.spinner();
                card_description(ui, "Closing ingest share…");
            });
        });
    }

    fn show_close_confirmation(
        &mut self,
        ui: &mut egui::Ui,
        session: &ActiveSession,
        credentials: &Option<Credentials>,
    ) {
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
            ConfirmationDialogAction::None => {}
            ConfirmationDialogAction::Cancel => {
                self.close_confirmation_open = false;
            }
            ConfirmationDialogAction::Confirm => {
                self.close_confirmation_open = false;
                if let Some(credentials) = credentials.clone() {
                    self.last_close = Some((credentials.clone(), session.clone()));
                    self.spawn_close(credentials, session.clone(), ui.ctx().clone());
                } else {
                    self.login_required = true;
                    ui.ctx().request_repaint();
                }
            }
        }
    }

    fn spawn_close(
        &mut self,
        credentials: Credentials,
        session: ActiveSession,
        ctx: egui::Context,
    ) {
        let service = Arc::clone(&self.service);
        self.close_task
            .spawn(move || service.close_session(&credentials, &session), ctx);
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
