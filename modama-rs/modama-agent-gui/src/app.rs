use eframe::egui::{self, vec2, Margin, Spacing};

use crate::{
    actions::Action,
    pages::{
        Page, PageConnecting, PageLogin, PageOverview, PageSelectCreateSession, PageSelectProject,
    },
    state::AgentAppState,
    styles,
};

pub struct AgentGuiApp {
    pub page: Page,
    pub app_state: AgentAppState,
}

impl Default for AgentGuiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentGuiApp {
    pub fn new() -> Self {
        Self {
            page: Page::Connecting(PageConnecting::new()),
            app_state: AgentAppState::new(),
        }
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::GoToLogin => {
                self.page = Page::Login(PageLogin::new());
            }
            Action::CreateSession => {
                if self.app_state.creds.is_some() {
                    self.page = Page::SelectProject(PageSelectProject::new());
                } else {
                    self.page = Page::Login(PageLogin::new());
                }
            }
            Action::LoginSuccess { creds } => {
                self.app_state.creds = Some(creds);
                self.page = Page::SelectProject(PageSelectProject::new());
            }
            Action::GoToSelectProject => {
                self.page = Page::SelectProject(PageSelectProject::new());
            }
            Action::SelectProject { project } => {
                self.page = Page::SelectCreateSession(PageSelectCreateSession::new(project));
            }
            Action::SelectSession { session } => {
                self.app_state.session = Some(session);
                self.page = Page::Overview(PageOverview::new());
            }
            Action::GoToOverview => {
                self.page = Page::Overview(PageOverview::new());
            }
        }
    }
}

impl eframe::App for AgentGuiApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        ui.style_mut().spacing = Spacing {
            item_spacing: vec2(
                styles::spacing::SCALE[3] * styles::spacing::UNIT_EGUI_POINTS,
                styles::spacing::SCALE[4] * styles::spacing::UNIT_EGUI_POINTS,
            ),
            window_margin: Margin {
                left: (styles::spacing::SCALE[3] * styles::spacing::UNIT_EGUI_POINTS) as i8,
                right: (styles::spacing::SCALE[3] * styles::spacing::UNIT_EGUI_POINTS) as i8,
                top: (styles::spacing::SCALE[4] * styles::spacing::UNIT_EGUI_POINTS) as i8,
                bottom: (styles::spacing::SCALE[4] * styles::spacing::UNIT_EGUI_POINTS) as i8,
            },
            ..Default::default()
        };

        // Poll current page for async completions
        let poll_action = self.page.poll();
        self.handle_action(poll_action);

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if matches!(self.page, Page::Login(_)) {
                let ui_action = self.page.ui(ui, frame, &self.app_state.session);
                self.handle_action(ui_action);
            } else {
                ui.add(
                    egui::Image::new(egui::include_image!("../assets/modama-tint.svg"))
                        .tint(styles::color::fg::VALUE.resolve_egui(ui.visuals()))
                        .max_width(256.0),
                );
                egui::Frame::default().inner_margin(27.0).show(ui, |ui| {
                    let ui_action = self.page.ui(ui, frame, &self.app_state.session);
                    self.handle_action(ui_action);
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Credentials;

    #[test]
    fn test_create_session_action_logged_out() {
        let mut app = AgentGuiApp::new();
        assert!(app.app_state.creds.is_none());

        app.handle_action(Action::CreateSession);
        assert!(matches!(app.page, Page::Login(_)));
    }

    #[test]
    fn test_create_session_action_logged_in() {
        let mut app = AgentGuiApp::new();
        app.app_state.creds = Some(Credentials {
            username: "user".to_string(),
            password: "pass".to_string(),
        });

        app.handle_action(Action::CreateSession);
        assert!(matches!(app.page, Page::SelectProject(_)));
    }
}
