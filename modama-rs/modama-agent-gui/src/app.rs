use eframe::egui::{self, Margin, vec2};
use std::sync::Arc;

use crate::{
    actions::{Action, LoginDestination, RetryAction, TaskFailure},
    components::{
        autostart_offer::{AutostartOffer, AutostartOfferAction},
        toast::{ToastAction, ToastState},
    },
    foreground_monitor::{ForegroundMonitor, ForegroundMonitorUpdate, events_url},
    helper_launch::ProcessLauncher,
    navigation_progress::NavigationProgress,
    pages::{
        Page, PageConnecting, PageLogin, PageOverview, PageSelectCreateSession, PageSelectProject,
    },
    service::{AgentService, HttpAgentService, StubAgentService},
    state::AgentAppState,
    styles,
    top_bar::TopBar,
};

pub struct AgentGuiApp {
    pub page: Page,
    pub app_state: AgentAppState,
    pub toast: ToastState,
    /// Shown at startup on Windows, unless autostart is set up or the offer was dismissed
    pub autostart_offer: Option<AutostartOffer>,
    top_bar: TopBar,
    navigation_progress: NavigationProgress,
    service: Arc<dyn AgentService>,
    foreground_monitor: Option<ForegroundMonitor>,
    /// The agent's base URL, if the app should follow the agent's stream and launch helpers
    agent_url: Option<String>,
}

impl Default for AgentGuiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentGuiApp {
    pub fn new() -> Self {
        Self::build(StubAgentService::shared(), None)
    }

    pub fn with_service(service: Arc<HttpAgentService>) -> Self {
        let agent_url = service.agent_url().to_owned();
        Self::build(service, Some(agent_url))
    }

    pub fn with_service_without_monitor(service: Arc<dyn AgentService>) -> Self {
        Self::build(service, None)
    }

    fn build(service: Arc<dyn AgentService>, agent_url: Option<String>) -> Self {
        Self {
            page: Page::Connecting(PageConnecting::with_service(Arc::clone(&service))),
            app_state: AgentAppState::new(),
            toast: ToastState::default(),
            autostart_offer: None,
            top_bar: TopBar::default(),
            navigation_progress: NavigationProgress::default(),
            service,
            foreground_monitor: None,
            agent_url,
        }
    }

    /// Offer to start the session helper at logon, unless that's set up or was declined
    #[must_use]
    pub fn with_autostart_offer(self) -> Self {
        #[cfg(windows)]
        if !crate::autostart::dismissed() && !crate::autostart::entry_exists() {
            return Self {
                autostart_offer: Some(AutostartOffer::default()),
                ..self
            };
        }
        self
    }

    fn handle_autostart_offer(&mut self, action: AutostartOfferAction) {
        match action {
            AutostartOfferAction::Enable => match self.enable_autostart() {
                Ok(()) => self.autostart_offer = None,
                Err(e) => {
                    log::warn!("could not set up autostart: {e}");
                    if let Some(offer) = &mut self.autostart_offer {
                        offer.error = Some(format!("Could not set up autostart: {e}"));
                    }
                }
            },
            AutostartOfferAction::Dismiss => {
                #[cfg(windows)]
                if let Err(e) = crate::autostart::dismiss() {
                    log::warn!("could not remember that the autostart offer was dismissed: {e}");
                }
                self.autostart_offer = None;
            }
        }
    }

    fn enable_autostart(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            let agent_url = self
                .agent_url
                .as_deref()
                .ok_or_else(|| "the agent URL is unknown".to_owned())?;
            let gui_exe = std::env::current_exe().map_err(|e| e.to_string())?;
            crate::autostart::enable(&gui_exe, agent_url).map_err(|e| e.message())
        }
        #[cfg(not(windows))]
        {
            Err("autostart is only supported on Windows".to_owned())
        }
    }

    fn overview_page(&self) -> Page {
        Page::Overview(PageOverview::with_service(Arc::clone(&self.service)))
    }

    fn login_page(&self, destination: LoginDestination) -> Page {
        Page::Login(PageLogin::with_service_and_destination(
            Arc::clone(&self.service),
            destination,
        ))
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
            Action::GoToLogin { destination } => {
                self.toast.clear();
                self.page = self.login_page(destination);
            }
            Action::CreateSession => {
                self.toast.clear();
                if let Some(page) = self.project_page() {
                    self.page = page;
                } else {
                    self.page = self.login_page(LoginDestination::SelectProject);
                }
            }
            Action::LoginSuccess { creds, destination } => {
                self.toast.clear();
                self.app_state.creds = Some(creds.clone());
                match destination {
                    LoginDestination::SelectProject => {
                        self.page = Page::SelectProject(PageSelectProject::with_service(
                            Arc::clone(&self.service),
                            creds,
                        ));
                    }
                    LoginDestination::ConfirmClose { share_id } => {
                        let mut page = PageOverview::with_service(Arc::clone(&self.service));
                        if matches!(
                            &self.app_state.foreground,
                            crate::state::ForegroundSession::Mounted(session)
                                if session.share_id == share_id
                        ) {
                            page.open_close_confirmation();
                        }
                        self.page = Page::Overview(page);
                    }
                    LoginDestination::Overview => {
                        self.page =
                            Page::Overview(PageOverview::with_service(Arc::clone(&self.service)))
                    }
                }
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
            Action::StartRequested {
                operation_id,
                project,
                session,
            } => {
                self.app_state.begin_start(operation_id, project, session);
            }
            Action::IngestCreated {
                operation_id,
                session,
            } => {
                if self.app_state.finish_start(operation_id, session) {
                    self.toast.clear();
                    self.page = self.overview_page();
                }
            }
            Action::StartFailed {
                operation_id,
                failure,
            } => {
                if self.app_state.fail_start(operation_id)
                    && let Some(failure) = failure
                {
                    self.handle_task_failure(failure);
                }
            }
            Action::ForegroundChanged(foreground) => {
                self.app_state.observe_foreground(foreground);
            }
            Action::ConnectionDropped => {
                self.app_state.connection_dropped = true;
            }
            Action::ConnectionRestored => {
                self.app_state.connection_dropped = false;
            }
            Action::GoToOverview => {
                self.toast.clear();
                self.page = self.overview_page();
            }
            Action::CloseRequested {
                operation_id,
                share_id,
            } => {
                self.app_state.begin_close(operation_id, share_id);
            }
            Action::CloseSessionSuccess { operation_id } => {
                if self.app_state.finish_close(operation_id) {
                    self.toast.clear();
                }
            }
            Action::CloseFailed {
                operation_id,
                failure,
            } => {
                if self.app_state.fail_close(operation_id) == Some(true) {
                    self.handle_task_failure(failure);
                }
            }
            Action::LogoutSuccess => {
                self.toast.clear();
                self.app_state.creds = None;
                self.page = self.overview_page();
            }
            Action::Connected {
                identity,
                foreground,
            } => {
                self.toast.clear();
                self.page = self.overview_page();
                self.app_state.identity = Some(identity);
                self.app_state.foreground = foreground;
                self.app_state.connection_dropped = false;
            }
            Action::TaskFailed(failure) => self.handle_task_failure(failure),
        }
    }

    fn handle_task_failure(&mut self, failure: TaskFailure) {
        match failure {
            TaskFailure::Stay { message, retry } => {
                self.toast.show_error(message, retry);
            }
            TaskFailure::ReturnToOverview { message, retry } => {
                self.page = self.overview_page();
                self.toast.show_error(message, retry);
            }
        }
    }

    fn start_monitor_if_needed(&mut self, ctx: &egui::Context) {
        let Some(agent_url) = &self.agent_url else {
            return;
        };
        if self.foreground_monitor.is_some() || self.app_state.identity.is_none() {
            return;
        }
        match events_url(agent_url) {
            Ok(url) => {
                self.foreground_monitor = Some(ForegroundMonitor::start(
                    url,
                    Box::new(ProcessLauncher::new(agent_url.clone())),
                    self.app_state.foreground.clone(),
                    ctx.clone(),
                ));
            }
            Err(e) => {
                log::error!("not following the agent: {e}");
                self.agent_url = None;
            }
        }
    }

    fn poll_foreground_monitor(&mut self) {
        while let Some(result) = self
            .foreground_monitor
            .as_ref()
            .map(ForegroundMonitor::try_recv)
        {
            match result {
                Ok(ForegroundMonitorUpdate::ForegroundChanged(foreground)) => {
                    self.handle_action(Action::ForegroundChanged(foreground));
                }
                Ok(ForegroundMonitorUpdate::ConnectionDropped) => {
                    self.handle_action(Action::ConnectionDropped);
                }
                Ok(ForegroundMonitorUpdate::ConnectionRestored) => {
                    self.handle_action(Action::ConnectionRestored);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.foreground_monitor = None;
                    self.handle_action(Action::ConnectionDropped);
                    break;
                }
            }
        }
    }

    fn handle_retry(&mut self, retry: RetryAction, ctx: egui::Context) {
        match retry {
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
                    let action = page.retry_create_ingest(project, session, ctx);
                    self.handle_action(action);
                }
            }
            RetryAction::CreateSession { project, session } => {
                if let Page::SelectCreateSession(page) = &mut self.page {
                    let action = page.retry_create_session(project, session, ctx);
                    self.handle_action(action);
                }
            }
            RetryAction::CloseSession => {
                if let Page::Overview(page) = &mut self.page {
                    let action = page.retry_close_session(ctx);
                    self.handle_action(action);
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
        self.start_monitor_if_needed(ui.ctx());
        self.poll_foreground_monitor();
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
                    let ui_action = self.page.ui(
                        ui,
                        frame,
                        &self.app_state.foreground,
                        self.app_state.pending_start(),
                        self.app_state.pending_close(),
                        &self.app_state.creds,
                    );
                    self.handle_action(ui_action);
                } else {
                    egui::Frame::default()
                        .inner_margin(styles::components::app_shell::padding::EGUI_POINTS)
                        .show(ui, |ui| {
                            let ui_action = self.page.ui(
                                ui,
                                frame,
                                &self.app_state.foreground,
                                self.app_state.pending_start(),
                                self.app_state.pending_close(),
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
        // the error toast takes the same place, and goes first
        if !self.toast.is_visible()
            && let Some(action) = self
                .autostart_offer
                .as_ref()
                .and_then(|offer| offer.show(ui.ctx()))
        {
            self.handle_autostart_offer(action);
        }

        self.navigation_progress.show(
            ui.ctx(),
            self.page.is_busy(),
            self.app_state.connection_dropped,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Credentials, OperationId};

    fn credentials() -> Credentials {
        Credentials {
            username: "user".to_owned(),
            password: "pass".to_owned(),
        }
    }

    fn mounted_session(share_id: &str) -> crate::state::ActiveSession {
        crate::state::ActiveSession {
            share_id: share_id.to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            mount_path: None,
        }
    }

    #[test]
    fn ordinary_login_success_opens_project_selection() {
        let mut app = AgentGuiApp::new();

        app.handle_action(Action::LoginSuccess {
            creds: credentials(),
            destination: LoginDestination::SelectProject,
        });

        assert!(matches!(app.page, Page::SelectProject(_)));
    }

    #[test]
    fn close_login_success_reopens_confirmation_for_the_same_mounted_share() {
        let mut app = AgentGuiApp::new();
        app.app_state.foreground =
            crate::state::ForegroundSession::Mounted(mounted_session("share"));

        app.handle_action(Action::LoginSuccess {
            creds: credentials(),
            destination: LoginDestination::ConfirmClose {
                share_id: "share".to_owned(),
            },
        });

        let Page::Overview(page) = &app.page else {
            panic!("close login did not return to the overview");
        };
        assert!(page.close_confirmation_is_open());
    }

    #[test]
    fn close_login_success_discards_stale_confirmation_destination() {
        for foreground in [
            crate::state::ForegroundSession::None,
            crate::state::ForegroundSession::Mounted(mounted_session("other")),
        ] {
            let mut app = AgentGuiApp::new();
            app.app_state.foreground = foreground;

            app.handle_action(Action::LoginSuccess {
                creds: credentials(),
                destination: LoginDestination::ConfirmClose {
                    share_id: "share".to_owned(),
                },
            });

            let Page::Overview(page) = &app.page else {
                panic!("close login did not return to the overview");
            };
            assert!(!page.close_confirmation_is_open());
        }
    }

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
        app.app_state.foreground =
            crate::state::ForegroundSession::Mounted(crate::state::ActiveSession {
                share_id: "share".to_owned(),
                project: "project".to_string(),
                session: "session".to_string(),
                mount_path: None,
            });

        app.handle_action(Action::LogoutSuccess);

        assert!(app.app_state.creds.is_none());
        assert!(matches!(
            app.app_state.foreground,
            crate::state::ForegroundSession::Mounted(_)
        ));
        assert!(matches!(app.page, Page::Overview(_)));
    }

    #[test]
    fn test_close_session_waits_for_authoritative_update_and_stays_on_overview() {
        let mut app = AgentGuiApp::new();
        app.page = Page::Overview(PageOverview::new());
        app.app_state.foreground =
            crate::state::ForegroundSession::Mounted(crate::state::ActiveSession {
                share_id: "share".to_owned(),
                project: "project".to_string(),
                session: "session".to_string(),
                mount_path: None,
            });

        let operation_id = OperationId::next();
        app.handle_action(Action::CloseRequested {
            operation_id,
            share_id: "share".to_owned(),
        });
        app.handle_action(Action::CloseSessionSuccess { operation_id });

        assert_eq!(app.app_state.pending_close(), Some("share"));
        assert!(matches!(
            app.app_state.foreground,
            crate::state::ForegroundSession::Mounted(_)
        ));
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
            foreground: crate::state::ForegroundSession::Mounted(session.clone()),
        });

        assert_eq!(
            app.app_state.foreground,
            crate::state::ForegroundSession::Mounted(session)
        );
        assert!(matches!(app.page, Page::Overview(_)));
    }

    #[test]
    fn monitor_update_replaces_foreground_and_completes_matching_start() {
        let mut app = AgentGuiApp::new();
        let pending = crate::state::PendingSession {
            share_id: "share".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
        };
        let operation_id = OperationId::next();
        app.handle_action(Action::StartRequested {
            operation_id,
            project: pending.project.clone(),
            session: pending.session.clone(),
        });
        app.handle_action(Action::IngestCreated {
            operation_id,
            session: pending.clone(),
        });

        app.handle_action(Action::ForegroundChanged(
            crate::state::ForegroundSession::Mounting(
                pending.clone(),
                "Waiting for the session helper to mount the share.".to_owned(),
            ),
        ));

        assert_eq!(app.app_state.pending_start(), None);
        assert_eq!(
            app.app_state.foreground,
            crate::state::ForegroundSession::Mounting(
                pending,
                "Waiting for the session helper to mount the share.".to_owned()
            )
        );
    }

    #[test]
    fn monitor_update_detects_external_session_deletion() {
        let mut app = AgentGuiApp::new();
        app.app_state.foreground =
            crate::state::ForegroundSession::Mounted(crate::state::ActiveSession {
                share_id: "external".to_owned(),
                project: "project".to_owned(),
                session: "session".to_owned(),
                mount_path: Some("/mnt/session".to_owned()),
            });

        app.handle_action(Action::ForegroundChanged(
            crate::state::ForegroundSession::None,
        ));

        assert_eq!(
            app.app_state.foreground,
            crate::state::ForegroundSession::None
        );
    }

    #[test]
    fn monitor_connection_events_track_the_latest_attempt() {
        let mut app = AgentGuiApp::new();

        app.handle_action(Action::ConnectionDropped);
        assert!(app.app_state.connection_dropped);

        app.handle_action(Action::ConnectionRestored);
        assert!(!app.app_state.connection_dropped);
    }

    #[test]
    fn pending_start_survives_unrelated_authoritative_updates() {
        let mut app = AgentGuiApp::new();
        let operation_id = OperationId::next();
        app.handle_action(Action::StartRequested {
            operation_id,
            project: "project".to_owned(),
            session: "session".to_owned(),
        });
        app.handle_action(Action::IngestCreated {
            operation_id,
            session: crate::state::PendingSession {
                share_id: "share".to_owned(),
                project: "project".to_owned(),
                session: "session".to_owned(),
            },
        });
        app.handle_action(Action::ForegroundChanged(
            crate::state::ForegroundSession::None,
        ));

        assert_eq!(
            app.app_state
                .pending_start()
                .map(|session| session.share_id.as_str()),
            Some("share")
        );
        assert_eq!(
            app.app_state.foreground,
            crate::state::ForegroundSession::None
        );
    }

    #[test]
    fn foreground_gone_after_close_clears_local_close_progress() {
        let mut app = AgentGuiApp::new();
        let session = crate::state::ActiveSession {
            share_id: "share".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            mount_path: Some("/mnt/session".to_owned()),
        };
        app.app_state.foreground = crate::state::ForegroundSession::Mounted(session.clone());
        let operation_id = OperationId::next();
        app.handle_action(Action::CloseRequested {
            operation_id,
            share_id: session.share_id.clone(),
        });

        app.handle_action(Action::ForegroundChanged(
            crate::state::ForegroundSession::None,
        ));

        assert_eq!(app.app_state.pending_close(), None);
        assert_eq!(
            app.app_state.foreground,
            crate::state::ForegroundSession::None
        );
    }

    #[test]
    fn mounted_monitor_update_before_start_completion_does_not_leave_stale_pending_state() {
        let mut app = AgentGuiApp::new();
        let pending = crate::state::PendingSession {
            share_id: "share".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
        };
        let mounted = crate::state::ActiveSession {
            share_id: pending.share_id.clone(),
            project: pending.project.clone(),
            session: pending.session.clone(),
            mount_path: Some("/mnt/session".to_owned()),
        };
        let operation_id = OperationId::next();
        app.handle_action(Action::StartRequested {
            operation_id,
            project: pending.project.clone(),
            session: pending.session.clone(),
        });

        // The permanent monitor wins the race and observes the completed mount first.
        app.handle_action(Action::ForegroundChanged(
            crate::state::ForegroundSession::Mounted(mounted),
        ));

        // The mounted share can disappear again before the page task is polled.
        app.handle_action(Action::ForegroundChanged(
            crate::state::ForegroundSession::None,
        ));

        // The late task completion must not recreate local pending state.
        app.handle_action(Action::IngestCreated {
            operation_id,
            session: pending,
        });

        assert_eq!(app.app_state.pending_start(), None);
        assert_eq!(
            app.app_state.foreground,
            crate::state::ForegroundSession::None
        );
    }
}
