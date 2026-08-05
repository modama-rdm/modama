use eframe::egui::{self, vec2, Margin};
use std::sync::Arc;

use crate::{
    actions::{Action, RetryAction, TaskFailure},
    components::toast::{ToastAction, ToastState},
    pages::{
        Page, PageConnecting, PageLogin, PageOverview, PageSelectCreateSession, PageSelectProject,
    },
    service::{AgentService, StubAgentService},
    state::AgentAppState,
    styles,
    top_bar::TopBar,
};

pub struct AgentGuiApp {
    pub page: Page,
    pub app_state: AgentAppState,
    pub toast: ToastState,
    top_bar: TopBar,
    service: Arc<dyn AgentService>,
}

impl Default for AgentGuiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentGuiApp {
    pub fn new() -> Self {
        Self::with_service(StubAgentService::shared())
    }

    pub fn with_service(service: Arc<dyn AgentService>) -> Self {
        Self {
            page: Page::Connecting(PageConnecting::with_service(Arc::clone(&service))),
            app_state: AgentAppState::new(),
            toast: ToastState::default(),
            top_bar: TopBar::default(),
            service,
        }
    }

    fn overview_page(&self) -> Page {
        Page::Overview(PageOverview::with_service(Arc::clone(&self.service)))
    }

    fn login_page(&self) -> Page {
        Page::Login(PageLogin::with_service(Arc::clone(&self.service)))
    }

    fn project_page(&self) -> Option<Page> {
        self.app_state.creds.clone().map(|credentials| {
            Page::SelectProject(PageSelectProject::with_service(
                Arc::clone(&self.service),
                credentials,
            ))
        })
    }

    fn session_page(&self, project: String) -> Option<Page> {
        self.app_state.creds.clone().map(|credentials| {
            Page::SelectCreateSession(PageSelectCreateSession::with_service(
                project,
                Arc::clone(&self.service),
                credentials,
            ))
        })
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::GoToLogin => {
                self.toast.clear();
                self.page = self.login_page();
            }
            Action::CreateSession => {
                self.toast.clear();
                if let Some(page) = self.project_page() {
                    self.page = page;
                } else {
                    self.page = self.login_page();
                }
            }
            Action::LoginSuccess { creds } => {
                self.toast.clear();
                self.app_state.creds = Some(creds.clone());
                self.page = Page::SelectProject(PageSelectProject::with_service(
                    Arc::clone(&self.service),
                    creds,
                ));
            }
            Action::GoToSelectProject => {
                self.toast.clear();
                if let Some(page) = self.project_page() {
                    self.page = page;
                }
            }
            Action::SelectProject { project } => {
                self.toast.clear();
                if let Some(page) = self.session_page(project) {
                    self.page = page;
                }
            }
            Action::SelectSession { session } => {
                self.toast.clear();
                self.app_state.session = Some(session);
                self.page = self.overview_page();
            }
            Action::GoToOverview => {
                self.toast.clear();
                self.page = self.overview_page();
            }
            Action::CloseSessionSuccess => {
                self.toast.clear();
                self.app_state.session = None;
            }
            Action::LogoutSuccess => {
                self.toast.clear();
                self.app_state.creds = None;
                self.page = self.overview_page();
            }
            Action::Connected { identity, session } => {
                self.toast.clear();
                self.page = self.overview_page();
                self.app_state.identity = Some(identity);
                self.app_state.session = session;
            }
            Action::TaskFailed(failure) => match failure {
                TaskFailure::Stay { message, retry } => {
                    self.toast.show_error(message, retry);
                }
                TaskFailure::ReturnToOverview { message, retry } => {
                    self.page = self.overview_page();
                    self.toast.show_error(message, retry);
                }
            },
        }
    }

    fn handle_retry(&mut self, retry: RetryAction, ctx: egui::Context) {
        match retry {
            RetryAction::Reconnect => {
                if let Page::Connecting(page) = &mut self.page {
                    page.retry(ctx);
                }
            }
            RetryAction::Login => {
                if let Page::Login(page) = &mut self.page {
                    page.retry(ctx);
                }
            }
            RetryAction::FetchProjects => {
                if let Some(page) = self.project_page() {
                    self.page = page;
                }
            }
            RetryAction::FetchSessions { project } => {
                if let Some(page) = self.session_page(project) {
                    self.page = page;
                }
            }
            RetryAction::CreateIngest { project, session } => {
                if let Page::SelectCreateSession(page) = &mut self.page {
                    page.retry_create_ingest(project, session, ctx);
                }
            }
            RetryAction::CreateSession { project, session } => {
                if let Page::SelectCreateSession(page) = &mut self.page {
                    page.retry_create_session(project, session, ctx);
                }
            }
            RetryAction::CloseSession => {
                if let Page::Overview(page) = &mut self.page {
                    page.retry_close_session(ctx);
                }
            }
        }
        self.toast.clear();
    }
}

impl eframe::App for AgentGuiApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        {
            let spacing = &mut ui.style_mut().spacing;
            spacing.item_spacing = vec2(
                styles::spacing::app_item_gap_x::EGUI_POINTS,
                styles::spacing::app_item_gap_y::EGUI_POINTS,
            );
            spacing.window_margin = Margin {
                left: styles::spacing::app_window_margin_x::EGUI_POINTS as i8,
                right: styles::spacing::app_window_margin_x::EGUI_POINTS as i8,
                top: styles::spacing::app_window_margin_y::EGUI_POINTS as i8,
                bottom: styles::spacing::app_window_margin_y::EGUI_POINTS as i8,
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
        let action = self.top_bar.show_inside(
            ui,
            username.as_deref(),
            &self.app_state.identity,
            page_busy,
            show_login_action,
        );
        self.handle_action(action);

        let central_panel_frame =
            egui::Frame::central_panel(ui.style()).inner_margin(Margin::symmetric(
                styles::spacing::app_window_margin_x::EGUI_POINTS as i8,
                styles::spacing::app_window_margin_y::EGUI_POINTS as i8,
            ));
        egui::CentralPanel::default()
            .frame(central_panel_frame)
            .show(ui, |ui| {
                if matches!(self.page, Page::Login(_)) {
                    let ui_action =
                        self.page
                            .ui(ui, frame, &self.app_state.session, &self.app_state.creds);
                    self.handle_action(ui_action);
                } else {
                    egui::Frame::default()
                        .inner_margin(styles::components::app_shell::padding::EGUI_POINTS)
                        .show(ui, |ui| {
                            let ui_action = self.page.ui(
                                ui,
                                frame,
                                &self.app_state.session,
                                &self.app_state.creds,
                            );
                            self.handle_action(ui_action);
                        });
                }
            });

        if let Some(action) = self.toast.show(ui.ctx()) {
            match action {
                ToastAction::Retry(retry) => self.handle_retry(retry, ui.ctx().clone()),
                ToastAction::Dismiss => self.toast.clear(),
            }
        }
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
    fn test_logout_clears_credentials_and_returns_to_overview_without_closing_session() {
        let mut app = AgentGuiApp::new();
        app.app_state.creds = Some(Credentials {
            username: "user".to_string(),
            password: "pass".to_string(),
        });
        app.app_state.session = Some(crate::state::ActiveSession {
            share_id: "share".to_owned(),
            project: "project".to_string(),
            session: "session".to_string(),
            mount_path: None,
        });

        app.handle_action(Action::LogoutSuccess);

        assert!(app.app_state.creds.is_none());
        assert!(app.app_state.session.is_some());
        assert!(matches!(app.page, Page::Overview(_)));
    }

    #[test]
    fn test_close_session_clears_session_and_stays_on_overview() {
        let mut app = AgentGuiApp::new();
        app.page = Page::Overview(PageOverview::new());
        app.app_state.session = Some(crate::state::ActiveSession {
            share_id: "share".to_owned(),
            project: "project".to_string(),
            session: "session".to_string(),
            mount_path: None,
        });

        app.handle_action(Action::CloseSessionSuccess);

        assert!(app.app_state.session.is_none());
        assert!(matches!(app.page, Page::Overview(_)));
    }

    #[test]
    fn connected_action_restores_the_foreground_session() {
        let mut app = AgentGuiApp::new();
        let session = crate::state::ActiveSession {
            share_id: "restored-share".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            mount_path: Some("/mnt/session".to_owned()),
        };

        app.handle_action(Action::Connected {
            identity: crate::state::AgentIdentity::Anonymous {
                hostname: "workstation".to_owned(),
            },
            session: Some(session.clone()),
        });

        assert_eq!(app.app_state.session, Some(session));
        assert!(matches!(app.page, Page::Overview(_)));
    }
}
