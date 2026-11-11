use eframe::egui::{self, Margin, vec2};
use modama_common::version::modama_git_version;
use std::sync::Arc;

#[cfg(feature = "autostart")]
use crate::components::autostart_offer::{AutostartOffer, AutostartOfferAction};
use crate::{
    actions::{Action, LoginDestination, RetryAction, TaskFailure},
    async_state::AsyncState,
    components::{
        buttons::ButtonVariant,
        dialogs::{ChoiceDialogAction, DialogChoice, choice_dialog},
        toast::{ToastAction, ToastState},
    },
    footer::show_footer,
    foreground_monitor::{ForegroundMonitor, events_url},
    helper_launch::{HelperLauncher, ProcessLauncher},
    navigation_progress::NavigationProgress,
    pages::overview::others_share_user,
    pages::{
        Page, PageConnecting, PageLogin, PageOverview, PageSelectCreateSession, PageSelectProject,
    },
    service::{AgentService, HttpAgentService, ServiceError, StubAgentService},
    state::{ActiveSession, AgentAppState, AgentView, ForegroundSession},
    styles,
    top_bar::TopBar,
};

pub struct AgentGuiApp {
    pub page: Page,
    pub app_state: AgentAppState,
    pub toast: ToastState,
    /// Shown at startup on Windows, unless autostart is set up or the offer was dismissed
    #[cfg(feature = "autostart")]
    pub autostart_offer: Option<AutostartOffer>,
    top_bar: TopBar,
    navigation_progress: NavigationProgress,
    service: Arc<dyn AgentService>,
    foreground_monitor: Option<ForegroundMonitor>,
    /// The agent's base URL, if the app should follow the agent's stream and launch helpers
    agent_url: Option<String>,
    /// Makes the launcher of session helpers for the agent's base URL, each time the stream is
    /// followed anew
    make_launcher: LauncherFactory,
    /// Shown in the footer
    version: &'static str,
    /// Open while the app asks, on logout, whether to stop the user's ingest shares
    logout_reminder: Option<LogoutReminder>,
    /// Stops the user's ingest shares, before logging out
    stop_before_logout: AsyncState<Result<(), ServiceError>>,
}

struct LogoutReminder {
    focus_cancel: bool,
}

#[derive(Clone, Copy)]
enum LogoutChoice {
    LogOut,
    StopAndLogOut,
}

const LOGOUT_REMINDER_ID: &str = "logout-reminder";

type LauncherFactory = Box<dyn Fn(&str) -> Box<dyn HelperLauncher>>;

/// The version shown by an app on the stub service (tests, guide screenshots). It's fixed, so
/// snapshots don't change with every commit, and it can't be mistaken for a release.
pub const STUB_VERSION: &str = "v0.0.0-example";

impl Default for AgentGuiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentGuiApp {
    pub fn new() -> Self {
        Self::build(StubAgentService::shared(), None, STUB_VERSION)
    }

    pub fn with_service(service: Arc<HttpAgentService>) -> Self {
        let agent_url = service.agent_url().to_owned();
        Self::build(service, Some(agent_url), modama_git_version())
    }

    pub fn with_service_without_monitor(service: Arc<dyn AgentService>) -> Self {
        Self::build(service, None, STUB_VERSION)
    }

    fn build(
        service: Arc<dyn AgentService>,
        agent_url: Option<String>,
        version: &'static str,
    ) -> Self {
        Self {
            page: Page::Connecting(PageConnecting::with_service(Arc::clone(&service))),
            app_state: AgentAppState::new(),
            toast: ToastState::default(),
            #[cfg(feature = "autostart")]
            autostart_offer: None,
            top_bar: TopBar::default(),
            navigation_progress: NavigationProgress::default(),
            service,
            foreground_monitor: None,
            agent_url,
            make_launcher: Box::new(|agent_url| Box::new(ProcessLauncher::new(agent_url))),
            version,
            logout_reminder: None,
            stop_before_logout: AsyncState::Idle,
        }
    }

    /// Launch session helpers with `make_launcher`'s launchers instead of starting our own
    /// executable, which in a test is the test binary
    #[must_use]
    pub fn with_helper_launcher(
        self,
        make_launcher: impl Fn(&str) -> Box<dyn HelperLauncher> + 'static,
    ) -> Self {
        Self {
            make_launcher: Box::new(make_launcher),
            ..self
        }
    }

    /// Offer to start the session helper at logon, unless that's set up or was declined
    #[cfg(feature = "autostart")]
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

    /// Start the navigation progress bar over, as if what it shows began in the next frame: for
    /// snapshots that stop egui's clock (`guide_screenshots::freeze_time`)
    #[cfg(feature = "guide-screenshots")]
    pub fn restart_navigation_progress(&mut self) {
        self.navigation_progress = NavigationProgress::default();
    }

    #[cfg(feature = "autostart")]
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

    #[cfg(feature = "autostart")]
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

    /// The overview, for every way back to it but a successful start: a share the user wanted
    /// closed after starting a new one stays open then
    fn overview_page(&mut self) -> Page {
        self.app_state.close_after_start = None;
        Page::Overview(PageOverview::with_service(Arc::clone(&self.service)))
    }

    fn login_page(&self, destination: LoginDestination) -> Page {
        Page::Login(PageLogin::with_service_and_destination(
            Arc::clone(&self.service),
            destination,
        ))
    }

    fn project_page(&self) -> Option<Page> {
        self.app_state.signed_in.clone().map(|signed_in| {
            Page::SelectProject(PageSelectProject::with_service(
                Arc::clone(&self.service),
                signed_in,
            ))
        })
    }

    fn session_page(&self, project: String) -> Option<Page> {
        self.app_state.signed_in.clone().map(|signed_in| {
            Page::SelectCreateSession(PageSelectCreateSession::with_service(
                project,
                Arc::clone(&self.service),
                signed_in,
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
                self.app_state.close_after_start = None;
                if let Some(page) = self.project_page() {
                    self.page = page;
                } else {
                    self.page = self.login_page(LoginDestination::SelectProject);
                }
            }
            Action::CreateSessionClosingPrevious { previous } => {
                self.toast.clear();
                self.app_state.close_after_start = Some(previous);
                if let Some(page) = self.project_page() {
                    self.page = page;
                } else {
                    self.page = self.login_page(LoginDestination::SelectProject);
                }
            }
            Action::LoginSuccess {
                signed_in,
                destination,
            } => {
                self.toast.clear();
                self.app_state.signed_in = Some(signed_in.clone());
                match destination {
                    LoginDestination::SelectProject => {
                        self.page = Page::SelectProject(PageSelectProject::with_service(
                            Arc::clone(&self.service),
                            signed_in,
                        ));
                    }
                    LoginDestination::ConfirmClose { share_id } => {
                        let mut page = PageOverview::with_service(Arc::clone(&self.service));
                        let other_mount = self
                            .app_state
                            .agent
                            .other_mounts
                            .iter()
                            .find(|mount| mount.share_id == share_id)
                            .cloned();
                        if matches!(
                            &self.app_state.agent.foreground,
                            crate::state::ForegroundSession::Mounted(session)
                                if session.share_id == share_id
                                    && crate::pages::overview::others_share_user(
                                        session.created_by.as_deref(),
                                        &signed_in,
                                    )
                                    .is_none()
                        ) {
                            page.open_close_confirmation();
                        } else if let Some(mount) = other_mount {
                            page.open_close_confirmation_for(mount);
                        }
                        self.page = Page::Overview(page);
                    }
                    LoginDestination::StartSession => {
                        if self.app_state.agent.foreground.share_id().is_some() {
                            let mut page = PageOverview::with_service(Arc::clone(&self.service));
                            page.open_start_dialog();
                            self.page = Page::Overview(page);
                        } else {
                            self.page = Page::SelectProject(PageSelectProject::with_service(
                                Arc::clone(&self.service),
                                signed_in,
                            ));
                        }
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
                let previous = self.app_state.close_after_start.take();
                if self.app_state.finish_start(operation_id, session) {
                    self.toast.clear();
                    self.page = match (previous, self.app_state.signed_in.clone()) {
                        (Some(previous), Some(signed_in)) => {
                            Page::Overview(PageOverview::closing_previous(
                                Arc::clone(&self.service),
                                signed_in,
                                previous,
                            ))
                        }
                        _ => self.overview_page(),
                    };
                }
            }
            Action::StartFailed {
                operation_id,
                failure,
            } => {
                // the previous share stays open when the new one couldn't be started
                self.app_state.close_after_start = None;
                if self.app_state.fail_start(operation_id)
                    && let Some(failure) = failure
                {
                    self.handle_task_failure(failure);
                }
            }
            Action::ForceRelease => {
                if let Some(monitor) = &self.foreground_monitor {
                    monitor.force_release();
                }
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
            Action::LogoutRequested => {
                if self.own_ingest_shares().is_empty() {
                    self.handle_action(Action::LogoutSuccess);
                } else {
                    self.logout_reminder = Some(LogoutReminder { focus_cancel: true });
                }
            }
            Action::LogoutSuccess => {
                self.toast.clear();
                self.service.logout();
                self.app_state.signed_in = None;
                self.app_state.close_after_start = None;
                self.page = self.overview_page();
            }
            Action::Connected {
                identity,
                foreground,
            } => {
                self.toast.clear();
                self.page = self.overview_page();
                self.app_state.identity = Some(identity);
                self.app_state.apply_agent_view(AgentView {
                    foreground,
                    connection_dropped: false,
                    ..self.app_state.agent.clone()
                });
            }
            Action::TaskFailed(failure) => self.handle_task_failure(failure),
        }
    }

    /// An authenticated call was refused: the agent or the management API ended the session
    /// (idle timeout, sign-out elsewhere). Drop it and show the login page.
    fn check_session_ended(&mut self) {
        if !self.service.take_session_ended() || self.app_state.signed_in.is_none() {
            return;
        }
        self.app_state.signed_in = None;
        self.app_state.close_after_start = None;
        self.toast.clear();
        let mut page = PageLogin::with_service_and_destination(
            Arc::clone(&self.service),
            LoginDestination::Overview,
        );
        page.login_error = Some("Your session has ended. Please sign in again.".to_owned());
        self.page = Page::Login(page);
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
                    (self.make_launcher)(agent_url),
                    self.app_state.agent.clone(),
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
        while let Some(monitor) = &self.foreground_monitor {
            match monitor.try_recv() {
                Ok(view) => self.app_state.apply_agent_view(view),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                // started again in the next frame, which waits for a repaint or input
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.foreground_monitor = None,
            }
        }
    }

    /// The ingest shares here that the signed-in user started: the foreground one (also
    /// while it mounts) and the others mounted for them. A share whose creator isn't known
    /// counts as the user's own, as when starting another ingest.
    fn own_ingest_shares(&self) -> Vec<ActiveSession> {
        let Some(signed_in) = &self.app_state.signed_in else {
            return Vec::new();
        };
        let foreground = match &self.app_state.agent.foreground {
            ForegroundSession::None => None,
            ForegroundSession::Mounted(session) => Some(session.clone()),
            ForegroundSession::Mounting(pending, _) => Some(ActiveSession {
                share_id: pending.share_id.clone(),
                project: pending.project.clone(),
                session: pending.session.clone(),
                mount_path: None,
                created_by: pending.created_by.clone(),
            }),
        };
        foreground
            .into_iter()
            .chain(self.app_state.agent.other_mounts.iter().cloned())
            .filter(|share| others_share_user(share.created_by.as_deref(), signed_in).is_none())
            .collect()
    }

    fn show_logout_reminder(&mut self, ui: &mut egui::Ui) {
        let Some(reminder) = &mut self.logout_reminder else {
            return;
        };
        let focus_cancel = std::mem::take(&mut reminder.focus_cancel);
        let message = if self.own_ingest_shares().len() > 1 {
            "Please remember to stop your ingest shares once you are done uploading data."
        } else {
            "Please remember to stop your ingest share once you are done uploading data."
        };
        let action = choice_dialog(
            ui.ctx(),
            ui.visuals(),
            LOGOUT_REMINDER_ID,
            "Log out?",
            message,
            &[
                DialogChoice {
                    label: "Log out",
                    variant: ButtonVariant::Primary,
                    value: LogoutChoice::LogOut,
                },
                DialogChoice {
                    label: "Stop and log out",
                    variant: ButtonVariant::Secondary,
                    value: LogoutChoice::StopAndLogOut,
                },
            ],
            focus_cancel,
        );
        match action {
            ChoiceDialogAction::None => {}
            ChoiceDialogAction::Cancel => self.logout_reminder = None,
            ChoiceDialogAction::Choose(LogoutChoice::LogOut) => {
                self.logout_reminder = None;
                self.handle_action(Action::LogoutSuccess);
            }
            ChoiceDialogAction::Choose(LogoutChoice::StopAndLogOut) => {
                self.logout_reminder = None;
                self.stop_and_log_out(ui.ctx().clone());
            }
        }
    }

    /// Stop the user's ingest shares, one after the other, and log out once all are stopped
    fn stop_and_log_out(&mut self, ctx: egui::Context) {
        let Some(signed_in) = self.app_state.signed_in.clone() else {
            return;
        };
        if self.stop_before_logout.is_pending() {
            return;
        }
        let shares = self.own_ingest_shares();
        let service = Arc::clone(&self.service);
        self.toast.clear();
        self.stop_before_logout.spawn(
            move || {
                shares
                    .iter()
                    .try_for_each(|share| service.close_session(&signed_in, share))
            },
            ctx,
        );
    }

    fn poll_stop_before_logout(&mut self) {
        let result = match self.stop_before_logout.poll() {
            Ok(task) => match task.take_ready() {
                Some(result) => result.map_err(|error| error.to_string()),
                None => return,
            },
            Err(error) => Err(error.to_string()),
        };
        self.stop_before_logout.reset();
        match result {
            Ok(()) => self.handle_action(Action::LogoutSuccess),
            Err(error) => {
                log::error!("stopping the ingest shares before logging out failed: {error}");
                self.toast.show_error(
                    "Could not stop the ingest share.".to_owned(),
                    RetryAction::StopAndLogOut,
                );
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
            RetryAction::StopAndLogOut => self.stop_and_log_out(ctx),
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

        // before the page sees its failed call: the session ended, so sign in again
        self.check_session_ended();

        // Poll current page for async completions
        let poll_action = self.page.poll();
        self.handle_action(poll_action);
        self.start_monitor_if_needed(ui.ctx());
        self.poll_foreground_monitor();
        let top_bar_action = self.top_bar.poll();
        self.handle_action(top_bar_action);

        let username = self
            .app_state
            .signed_in
            .as_ref()
            .map(|signed_in| signed_in.username.clone());
        self.poll_stop_before_logout();
        let page_busy = self.page.is_busy() || self.stop_before_logout.is_pending();
        let show_login_action = !matches!(self.page, Page::Login(_));
        let action = self.top_bar.show_inside(
            ui,
            username.as_deref(),
            &self.app_state.identity,
            page_busy,
            show_login_action,
        );
        self.handle_action(action);
        show_footer(ui, self.version);

        let central_panel_frame =
            egui::Frame::central_panel(ui.style()).inner_margin(Margin::symmetric(
                styles::spacing::app_window_margin_x::EGUI_POINTS as i8,
                styles::spacing::app_window_margin_y::EGUI_POINTS as i8,
            ));
        egui::CentralPanel::default()
            .frame(central_panel_frame)
            .show(ui, |ui| {
                if matches!(self.page, Page::Login(_)) {
                    let ui_action = self.page.ui(ui, frame, &self.app_state);
                    self.handle_action(ui_action);
                } else {
                    egui::Frame::default()
                        .inner_margin(styles::components::app_shell::padding::EGUI_POINTS)
                        .show(ui, |ui| {
                            let ui_action = self.page.ui(ui, frame, &self.app_state);
                            self.handle_action(ui_action);
                        });
                }
            });

        self.show_logout_reminder(ui);

        if let Some(action) = self.toast.show(ui.ctx()) {
            match action {
                ToastAction::Retry(retry) => self.handle_retry(retry, ui.ctx().clone()),
                ToastAction::Dismiss => self.toast.clear(),
            }
        }
        // the error toast takes the same place, and goes first
        #[cfg(feature = "autostart")]
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
            self.page.is_busy() || self.stop_before_logout.is_pending(),
            self.app_state.agent.connection_dropped,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{OperationId, SignedIn};

    fn signed_in() -> SignedIn {
        SignedIn {
            username: "user".to_owned(),
        }
    }

    fn mounted_session(share_id: &str) -> crate::state::ActiveSession {
        crate::state::ActiveSession {
            share_id: share_id.to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            mount_path: None,
            created_by: None,
        }
    }

    fn own_mounted(share_id: &str) -> crate::state::ActiveSession {
        crate::state::ActiveSession {
            created_by: Some("user".to_owned()),
            ..mounted_session(share_id)
        }
    }

    /// What the agent reports with `foreground` and nothing else
    fn foreground_view(foreground: crate::state::ForegroundSession) -> AgentView {
        AgentView {
            foreground,
            ..AgentView::default()
        }
    }

    fn start(app: &mut AgentGuiApp, share_id: &str) {
        let operation_id = OperationId::next();
        app.handle_action(Action::StartRequested {
            operation_id,
            project: "project".to_owned(),
            session: "new-session".to_owned(),
        });
        app.handle_action(Action::IngestCreated {
            operation_id,
            session: crate::state::PendingSession {
                share_id: share_id.to_owned(),
                project: "project".to_owned(),
                session: "new-session".to_owned(),
                created_by: None,
            },
        });
    }

    fn signed_in_with_open_share() -> (AgentGuiApp, crate::state::ActiveSession) {
        let mut app = AgentGuiApp::new();
        app.app_state.signed_in = Some(signed_in());
        let previous = own_mounted("previous");
        app.app_state.agent.foreground = crate::state::ForegroundSession::Mounted(previous.clone());
        (app, previous)
    }

    #[test]
    fn the_previous_share_is_closed_only_after_the_new_one_started() {
        let (mut app, previous) = signed_in_with_open_share();
        app.handle_action(Action::CreateSessionClosingPrevious {
            previous: previous.clone(),
        });
        assert!(matches!(app.page, Page::SelectProject(_)));
        assert_eq!(app.app_state.close_after_start, Some(previous.clone()));

        start(&mut app, "new");
        let Page::Overview(page) = &app.page else {
            panic!("a started session did not lead back to the overview");
        };
        assert_eq!(page.closes_previous(), Some(&previous));
        assert_eq!(app.app_state.close_after_start, None);
    }

    #[test]
    fn keeping_the_previous_share_open_closes_nothing() {
        let (mut app, _previous) = signed_in_with_open_share();
        app.handle_action(Action::CreateSession);
        start(&mut app, "new");
        let Page::Overview(page) = &app.page else {
            panic!("a started session did not lead back to the overview");
        };
        assert_eq!(page.closes_previous(), None);
    }

    #[test]
    fn the_previous_share_stays_open_if_the_start_fails_or_is_abandoned() {
        let (mut app, previous) = signed_in_with_open_share();
        app.handle_action(Action::CreateSessionClosingPrevious {
            previous: previous.clone(),
        });
        let operation_id = OperationId::next();
        app.handle_action(Action::StartRequested {
            operation_id,
            project: "project".to_owned(),
            session: "new-session".to_owned(),
        });
        app.handle_action(Action::StartFailed {
            operation_id,
            failure: None,
        });
        assert_eq!(app.app_state.close_after_start, None);

        app.handle_action(Action::CreateSessionClosingPrevious { previous });
        app.handle_action(Action::GoToOverview);
        assert_eq!(app.app_state.close_after_start, None);
    }

    #[test]
    fn signing_in_to_start_a_session_asks_first_if_a_share_is_open() {
        let mut app = AgentGuiApp::new();
        app.app_state.agent.foreground =
            crate::state::ForegroundSession::Mounted(own_mounted("previous"));
        app.handle_action(Action::LoginSuccess {
            signed_in: signed_in(),
            destination: LoginDestination::StartSession,
        });
        let Page::Overview(page) = &app.page else {
            panic!("signing in to start did not return to the overview");
        };
        assert!(page.start_dialog_is_open());

        let mut app = AgentGuiApp::new();
        app.handle_action(Action::LoginSuccess {
            signed_in: signed_in(),
            destination: LoginDestination::StartSession,
        });
        assert!(matches!(app.page, Page::SelectProject(_)));
    }

    #[test]
    fn signing_in_to_close_a_listed_share_asks_to_confirm_it() {
        let mut app = AgentGuiApp::new();
        app.app_state.apply_agent_view(AgentView {
            foreground: crate::state::ForegroundSession::Mounted(own_mounted("new")),
            other_mounts: vec![own_mounted("old")],
            ..AgentView::default()
        });
        app.handle_action(Action::LoginSuccess {
            signed_in: signed_in(),
            destination: LoginDestination::ConfirmClose {
                share_id: "old".to_owned(),
            },
        });
        let Page::Overview(page) = &app.page else {
            panic!("signing in to close did not return to the overview");
        };
        assert!(page.close_confirmation_is_open());
        assert_eq!(page.closes_mount(), Some(&own_mounted("old")));
    }

    #[test]
    fn ordinary_login_success_opens_project_selection() {
        let mut app = AgentGuiApp::new();

        app.handle_action(Action::LoginSuccess {
            signed_in: signed_in(),
            destination: LoginDestination::SelectProject,
        });

        assert!(matches!(app.page, Page::SelectProject(_)));
    }

    #[test]
    fn close_login_success_reopens_confirmation_for_the_same_mounted_share() {
        let mut app = AgentGuiApp::new();
        app.app_state.agent.foreground =
            crate::state::ForegroundSession::Mounted(mounted_session("share"));

        app.handle_action(Action::LoginSuccess {
            signed_in: signed_in(),
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
            app.app_state.agent.foreground = foreground;

            app.handle_action(Action::LoginSuccess {
                signed_in: signed_in(),
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
        assert!(app.app_state.signed_in.is_none());

        app.handle_action(Action::CreateSession);
        assert!(matches!(app.page, Page::Login(_)));
    }

    #[test]
    fn test_create_session_action_logged_in() {
        let mut app = AgentGuiApp::new();
        app.app_state.signed_in = Some(SignedIn {
            username: "user".to_string(),
        });

        app.handle_action(Action::CreateSession);
        assert!(matches!(app.page, Page::SelectProject(_)));
    }

    #[test]
    fn test_logout_clears_credentials_and_returns_to_overview_without_closing_session() {
        let mut app = AgentGuiApp::new();
        app.app_state.signed_in = Some(SignedIn {
            username: "user".to_string(),
        });
        app.app_state.agent.foreground =
            crate::state::ForegroundSession::Mounted(crate::state::ActiveSession {
                share_id: "share".to_owned(),
                project: "project".to_string(),
                session: "session".to_string(),
                mount_path: None,
                created_by: None,
            });

        app.handle_action(Action::LogoutSuccess);

        assert!(app.app_state.signed_in.is_none());
        assert!(matches!(
            app.app_state.agent.foreground,
            crate::state::ForegroundSession::Mounted(_)
        ));
        assert!(matches!(app.page, Page::Overview(_)));
    }

    #[test]
    fn an_ended_session_leads_to_the_login_page() {
        let service = Arc::new(crate::service::StubAgentService::new());
        let mut app = AgentGuiApp::with_service_without_monitor(Arc::clone(&service) as _);
        app.app_state.signed_in = Some(signed_in());
        app.page = app.overview_page();

        // nothing happened yet:
        app.check_session_ended();
        assert!(matches!(app.page, Page::Overview(_)));

        service.end_session();
        app.check_session_ended();
        assert!(app.app_state.signed_in.is_none());
        let Page::Login(page) = &app.page else {
            panic!("an ended session did not lead to the login page");
        };
        assert_eq!(
            page.login_error.as_deref(),
            Some("Your session has ended. Please sign in again.")
        );
    }

    #[test]
    fn test_close_session_waits_for_authoritative_update_and_stays_on_overview() {
        let mut app = AgentGuiApp::new();
        app.page = Page::Overview(PageOverview::new());
        app.app_state.agent.foreground =
            crate::state::ForegroundSession::Mounted(crate::state::ActiveSession {
                share_id: "share".to_owned(),
                project: "project".to_string(),
                session: "session".to_string(),
                mount_path: None,
                created_by: None,
            });

        let operation_id = OperationId::next();
        app.handle_action(Action::CloseRequested {
            operation_id,
            share_id: "share".to_owned(),
        });
        app.handle_action(Action::CloseSessionSuccess { operation_id });

        assert_eq!(app.app_state.pending_close(), Some("share"));
        assert!(matches!(
            app.app_state.agent.foreground,
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
            created_by: None,
        };

        app.handle_action(Action::Connected {
            identity: crate::state::AgentIdentity::Anonymous {
                hostname: "workstation".to_owned(),
            },
            foreground: crate::state::ForegroundSession::Mounted(session.clone()),
        });

        assert_eq!(
            app.app_state.agent.foreground,
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
            created_by: None,
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

        app.app_state
            .apply_agent_view(foreground_view(crate::state::ForegroundSession::Mounting(
                pending.clone(),
                "Waiting for the session helper to mount the ingest share.".to_owned(),
            )));

        assert_eq!(app.app_state.pending_start(), None);
        assert_eq!(
            app.app_state.agent.foreground,
            crate::state::ForegroundSession::Mounting(
                pending,
                "Waiting for the session helper to mount the ingest share.".to_owned()
            )
        );
    }

    #[test]
    fn monitor_update_detects_external_session_deletion() {
        let mut app = AgentGuiApp::new();
        app.app_state.agent.foreground =
            crate::state::ForegroundSession::Mounted(crate::state::ActiveSession {
                share_id: "external".to_owned(),
                project: "project".to_owned(),
                session: "session".to_owned(),
                mount_path: Some("/mnt/session".to_owned()),
                created_by: None,
            });

        app.app_state
            .apply_agent_view(foreground_view(crate::state::ForegroundSession::None));

        assert_eq!(
            app.app_state.agent.foreground,
            crate::state::ForegroundSession::None
        );
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
                created_by: None,
            },
        });
        app.app_state
            .apply_agent_view(foreground_view(crate::state::ForegroundSession::None));

        assert_eq!(
            app.app_state
                .pending_start()
                .map(|session| session.share_id.as_str()),
            Some("share")
        );
        assert_eq!(
            app.app_state.agent.foreground,
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
            created_by: None,
        };
        app.app_state.agent.foreground = crate::state::ForegroundSession::Mounted(session.clone());
        let operation_id = OperationId::next();
        app.handle_action(Action::CloseRequested {
            operation_id,
            share_id: session.share_id.clone(),
        });

        app.app_state
            .apply_agent_view(foreground_view(crate::state::ForegroundSession::None));

        assert_eq!(app.app_state.pending_close(), None);
        assert_eq!(
            app.app_state.agent.foreground,
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
            created_by: None,
        };
        let mounted = crate::state::ActiveSession {
            share_id: pending.share_id.clone(),
            project: pending.project.clone(),
            session: pending.session.clone(),
            mount_path: Some("/mnt/session".to_owned()),
            created_by: None,
        };
        let operation_id = OperationId::next();
        app.handle_action(Action::StartRequested {
            operation_id,
            project: pending.project.clone(),
            session: pending.session.clone(),
        });

        // The permanent monitor wins the race and observes the completed mount first.
        app.app_state
            .apply_agent_view(foreground_view(crate::state::ForegroundSession::Mounted(
                mounted,
            )));

        // The mounted share can disappear again before the page task is polled.
        app.app_state
            .apply_agent_view(foreground_view(crate::state::ForegroundSession::None));

        // The late task completion must not recreate local pending state.
        app.handle_action(Action::IngestCreated {
            operation_id,
            session: pending,
        });

        assert_eq!(app.app_state.pending_start(), None);
        assert_eq!(
            app.app_state.agent.foreground,
            crate::state::ForegroundSession::None
        );
    }

    #[test]
    fn a_close_in_flight_stays_pending_while_its_share_moves_to_the_other_mounts() {
        let mut app = AgentGuiApp::new();
        let old = crate::state::ForegroundSession::Mounted(mounted_session("old"));
        app.app_state.apply_agent_view(foreground_view(old));
        let operation_id = OperationId::next();
        app.handle_action(Action::CloseRequested {
            operation_id,
            share_id: "old".to_owned(),
        });

        // another share takes the foreground before the close is answered
        let new = crate::state::ForegroundSession::Mounted(mounted_session("new"));
        app.app_state.apply_agent_view(AgentView {
            other_mounts: vec![mounted_session("old")],
            ..foreground_view(new)
        });
        assert_eq!(app.app_state.pending_close(), Some("old"));

        // so a failure is still shown
        app.handle_action(Action::CloseFailed {
            operation_id,
            failure: TaskFailure::Stay {
                message: "could not close".to_owned(),
                retry: RetryAction::CloseSession,
            },
        });
        assert!(app.toast.is_visible());
    }
}
