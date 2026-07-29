use eframe::egui::{self, text::LayoutJob, Label, RichText, TextFormat};

use crate::{actions::Action, buttons::primary_button, state::ActiveSession, styles};

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
        ui.add(Label::new(
            RichText::new("Overview").font(styles::typography::heading::font_id()),
        ));

        if let Some(session) = session {
            let prose_format = TextFormat {
                font_id: styles::typography::body::font_id(),
                color: styles::color::on_surface_variant::VALUE.resolve_egui(ui.visuals()),
                ..Default::default()
            };
            let value_format = TextFormat {
                font_id: styles::typography::mono_medium::font_id(),
                color: styles::color::on_surface::VALUE.resolve_egui(ui.visuals()),
                ..Default::default()
            };
            let mut job = LayoutJob::default();
            job.append("Active session ", 0.0, prose_format.clone());
            job.append(&session.session, 0.0, value_format.clone());
            job.append(" in project ", 0.0, prose_format);
            job.append(&session.project, 0.0, value_format);
            ui.add(Label::new(job));
        }

        if ui.add(primary_button("Create Session", ui)).clicked() {
            return Action::CreateSession;
        }

        Action::None
    }

    pub fn poll(&mut self) -> Action {
        Action::None
    }

    pub fn is_busy(&self) -> bool {
        false
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
