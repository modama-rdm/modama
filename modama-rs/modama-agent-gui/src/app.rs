use eframe::egui::{self, vec2, Margin};

use crate::{
    actions::Action,
    pages::{
        Page, PageConnecting, PageLogin, PageOverview, PageSelectCreateSession, PageSelectProject,
    },
    state::AgentAppState,
    styles,
    top_bar::TopBar,
};

pub struct AgentGuiApp {
    pub page: Page,
    pub app_state: AgentAppState,
    top_bar: TopBar,
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
            top_bar: TopBar::default(),
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
            Action::LogoutSuccess => {
                self.app_state.creds = None;
                self.app_state.session = None;
                self.page = Page::Overview(PageOverview::new());
            }
        }
    }
}

impl eframe::App for AgentGuiApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        {
            let spacing = &mut ui.style_mut().spacing;
            spacing.item_spacing = vec2(
                styles::spacing::SCALE[3] * styles::spacing::UNIT_EGUI_POINTS,
                styles::spacing::SCALE[4] * styles::spacing::UNIT_EGUI_POINTS,
            );
            spacing.window_margin = Margin {
                left: (styles::spacing::SCALE[3] * styles::spacing::UNIT_EGUI_POINTS) as i8,
                right: (styles::spacing::SCALE[3] * styles::spacing::UNIT_EGUI_POINTS) as i8,
                top: (styles::spacing::SCALE[4] * styles::spacing::UNIT_EGUI_POINTS) as i8,
                bottom: (styles::spacing::SCALE[4] * styles::spacing::UNIT_EGUI_POINTS) as i8,
            };
        }

        // Poll current page for async completions
        let poll_action = self.page.poll();
        self.handle_action(poll_action);
        let top_bar_action = self.top_bar.poll();
        self.handle_action(top_bar_action);

        let username = self
            .app_state
            .creds
            .as_ref()
            .map(|credentials| credentials.username.clone());
        let page_busy = self.page.is_busy();
        let show_login_action = !matches!(self.page, Page::Login(_));
        let action =
            self.top_bar
                .show_inside(ui, username.as_deref(), page_busy, show_login_action);
        self.handle_action(action);

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if matches!(self.page, Page::Login(_)) {
                let ui_action = self.page.ui(ui, frame, &self.app_state.session);
                self.handle_action(ui_action);
            } else {
                egui::Frame::default()
                    .inner_margin(styles::components::app_shell::padding::EGUI_POINTS)
                    .show(ui, |ui| {
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

    #[test]
    fn test_logout_clears_state_and_returns_to_overview() {
        let mut app = AgentGuiApp::new();
        app.app_state.creds = Some(Credentials {
            username: "user".to_string(),
            password: "pass".to_string(),
        });
        app.app_state.session = Some(crate::state::ActiveSession {
            project: "project".to_string(),
            session: "session".to_string(),
        });

        app.handle_action(Action::LogoutSuccess);

        assert!(app.app_state.creds.is_none());
        assert!(app.app_state.session.is_none());
        assert!(matches!(app.page, Page::Overview(_)));
    }
}
