use eframe::egui::{
    self, text::LayoutJob, FontFamily, FontId, Label, RichText, TextFormat,
};

use crate::{actions::Action, buttons::action_button, state::ActiveSession};

pub struct PageOverview;

impl Default for PageOverview {
    fn default() -> Self {
        Self::new()
    }
}

impl PageOverview {
    pub fn new() -> Self {
        Self
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
        session: &Option<ActiveSession>,
    ) -> Action {
        ui.add(Label::new(RichText::new("Overview").size(25.0).strong()));

        if let Some(session) = session {
            let mut job = LayoutJob::default();
            job.append("Active session ", 0.0, TextFormat::default());
            job.append(
                &session.session,
                0.0,
                TextFormat {
                    font_id: FontId::new(14.0, FontFamily::Proportional),
                    italics: true,
                    ..Default::default()
                },
            );
            job.append(" in project ", 0.0, TextFormat::default());
            job.append(
                &session.project,
                0.0,
                TextFormat {
                    font_id: FontId::new(14.0, FontFamily::Proportional),
                    italics: true,
                    ..Default::default()
                },
            );
            ui.add(Label::new(job));
        }

        if ui.add(action_button("Create Session")).clicked() {
            return Action::CreateSession;
        }

        Action::None
    }

    pub fn poll(&mut self) -> Action {
        Action::None
    }
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
