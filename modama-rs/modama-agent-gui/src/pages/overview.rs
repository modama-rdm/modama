use eframe::egui::{self, Label, RichText};
use std::time::Duration;

use crate::{
    actions::Action,
    async_state::AsyncState,
    buttons::{danger_outline_button, loading_button, primary_button, ButtonVariant},
    components::{destructive_confirmation_dialog, page_heading, ConfirmationDialogAction},
    state::ActiveSession,
    styles,
};

const CLOSE_CONFIRMATION_ID: &str = "close-session-confirmation";

#[derive(Debug, Clone)]
pub struct CloseSessionRequest;

impl CloseSessionRequest {
    pub fn execute(&self) {
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub struct PageOverview {
    pub close_task: AsyncState<()>,
    close_confirmation_open: bool,
    focus_confirmation_cancel: bool,
}

impl Default for PageOverview {
    fn default() -> Self {
        Self::new()
    }
}

impl PageOverview {
    pub fn new() -> Self {
        Self {
            close_task: AsyncState::Idle,
            close_confirmation_open: false,
            focus_confirmation_cancel: false,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
        session: &Option<ActiveSession>,
    ) -> Action {
        page_heading(ui, "Overview");
        ui.add_space(styles::content_card_top_margin_adjustment());

        if let Some(session) = session {
            self.show_active_session_card(ui, session);
            if self.close_confirmation_open {
                self.show_close_confirmation(ui, session);
            }
        } else {
            self.close_confirmation_open = false;
            if self.show_create_card(ui) {
                return Action::CreateSession;
            }
        }

        Action::None
    }

    pub fn poll(&mut self) -> Action {
        if self.close_task.poll().take_ready().is_some() {
            Action::CloseSessionSuccess
        } else {
            Action::None
        }
    }

    pub fn is_busy(&self) -> bool {
        self.close_task.is_pending()
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
            clicked = ui.add(primary_button("Create Session", ui)).clicked();
        });
        clicked
    }

    fn show_active_session_card(&mut self, ui: &mut egui::Ui, session: &ActiveSession) {
        let is_closing = self.close_task.is_pending();
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
            if response.clicked() {
                self.close_confirmation_open = true;
                self.focus_confirmation_cancel = true;
            }
        });
    }

    fn show_close_confirmation(&mut self, ui: &mut egui::Ui, session: &ActiveSession) {
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
                self.close_task
                    .spawn(move || CloseSessionRequest.execute(), ui.ctx().clone());
            }
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
            .color(styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals())),
    ));
}

fn session_detail(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.add_space(styles::spacing::field_label_gap::EGUI_POINTS);
    ui.add(Label::new(
        RichText::new(label)
            .font(styles::typography::supporting_text::font_id())
            .color(styles::color::on_surface_dim::VALUE.resolve_egui(ui.visuals())),
    ));
    ui.add(Label::new(
        RichText::new(value).font(styles::typography::mono_medium::font_id()),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_session_request_completes() {
        CloseSessionRequest.execute();
    }

    #[test]
    fn test_overview_poll_returns_none() {
        let mut page = PageOverview::new();
        assert_eq!(page.poll(), Action::None);
    }
}
