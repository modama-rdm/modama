use eframe::egui::{self, RichText, TextEdit};
use std::sync::Arc;

use crate::{
    actions::{Action, LoginDestination, RetryAction, TaskFailure},
    async_state::AsyncState,
    components::{
        buttons::{primary_button, primary_loading_button},
        display_error,
        forms::labeled_text_input,
    },
    service::{AgentService, ServiceError, StubAgentService},
    state::Credentials,
    styles,
};

#[derive(Clone)]
pub struct LoginRequest {
    pub credentials: Credentials,
    service: Arc<dyn AgentService>,
}

impl LoginRequest {
    pub fn execute(&self) -> Result<Credentials, ServiceError> {
        self.service.login(&self.credentials)?;
        Ok(self.credentials.clone())
    }
}

pub struct PageLogin {
    pub username: String,
    pub password: String,
    pub login_error: Option<String>,
    pub login_task: AsyncState<Result<Credentials, ServiceError>>,
    service: Arc<dyn AgentService>,
    focus_username_on_entry: bool,
    destination: LoginDestination,
}

impl Default for PageLogin {
    fn default() -> Self {
        Self::new()
    }
}

impl PageLogin {
    pub fn new() -> Self {
        Self::with_service(StubAgentService::shared())
    }

    pub fn with_service(service: Arc<dyn AgentService>) -> Self {
        Self::with_service_and_destination(service, LoginDestination::SelectProject)
    }

    pub fn with_service_and_destination(
        service: Arc<dyn AgentService>,
        destination: LoginDestination,
    ) -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            login_error: None,
            login_task: AsyncState::Idle,
            service,
            focus_username_on_entry: true,
            destination,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        egui::Area::new(egui::Id::new("login-card"))
            .fixed_pos(ui.max_rect().center())
            .pivot(egui::Align2::CENTER_CENTER)
            .show(ui.ctx(), |ui| self.show_login_block(ui))
            .inner
    }

    fn show_login_block(&mut self, ui: &mut egui::Ui) -> Action {
        let is_logging_in = self.login_task.is_pending();
        let card_width = styles::components::login_card::width::EGUI_POINTS;

        ui.set_min_width(card_width);
        ui.set_max_width(card_width);
        ui.vertical_centered(|ui| {
            styles::card_frame(ui.visuals()).show(ui, |ui| {
                ui.style_mut().spacing.item_spacing.y = 0.0;
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    self.show_card_content(ui)
                });
            });

            ui.add_space(styles::spacing::login_back_link_gap::EGUI_POINTS);
            let link = egui::Button::new(
                egui::RichText::new("← Back to overview")
                    .font(styles::typography::supporting_text::font_id())
                    .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
            )
            .frame(false);
            let response = ui.add_enabled(!is_logging_in, link);
            styles::paint_focus_ring(ui, &response);
            if response.clicked() {
                Action::GoToOverview
            } else {
                Action::None
            }
        })
        .inner
    }

    fn show_card_content(&mut self, ui: &mut egui::Ui) {
        let is_logging_in = self.login_task.is_pending();

        show_login_header(ui);
        ui.add_space(styles::spacing::header_bottom_gap::EGUI_POINTS);

        if let Some(error) = self.login_error.as_deref() {
            display_error(ui, error);
            ui.add_space(styles::spacing::login_error_gap::EGUI_POINTS);
        }

        let submit_from_input = self.show_fields(ui, is_logging_in);
        self.show_submit(ui, is_logging_in, submit_from_input);
    }

    fn show_fields(&mut self, ui: &mut egui::Ui, is_logging_in: bool) -> bool {
        let lost_focus = ui
            .add_enabled_ui(!is_logging_in, |ui| {
                ui.style_mut().spacing.item_spacing.y = 0.0;
                let username_response =
                    labeled_text_input(ui, "Username", TextEdit::singleline(&mut self.username));
                if self.focus_username_on_entry && username_response.enabled() {
                    username_response.request_focus();
                    self.focus_username_on_entry = false;
                }
                let user_lost_focus = username_response.lost_focus();
                ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
                let pw_lost_focus = labeled_text_input(
                    ui,
                    "Password",
                    TextEdit::singleline(&mut self.password).password(true),
                )
                .lost_focus();
                user_lost_focus || pw_lost_focus
            })
            .inner;
        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
        !is_logging_in && enter_pressed && lost_focus
    }

    fn show_submit(&mut self, ui: &mut egui::Ui, is_logging_in: bool, submit_from_input: bool) {
        ui.add_space(styles::spacing::form_field_gap::EGUI_POINTS);
        ui.add_space(styles::spacing::button_margin_top::EGUI_POINTS);
        ui.vertical_centered(|ui| {
            let button_size = egui::vec2(ui.available_width(), 0.0);
            let response = if is_logging_in {
                primary_loading_button(ui, "Authenticating…", button_size)
            } else {
                ui.add(primary_button("Sign in", ui).min_size(button_size))
            };
            if response.clicked() || submit_from_input {
                self.start_login(ui.ctx().clone());
            }
        });
    }

    fn start_login(&mut self, ctx: egui::Context) {
        self.login_error = None;
        let req = LoginRequest {
            credentials: Credentials {
                username: self.username.clone(),
                password: self.password.clone(),
            },
            service: Arc::clone(&self.service),
        };
        self.login_task.spawn(move || req.execute(), ctx);
    }

    pub fn poll(&mut self) -> Action {
        match self.login_task.poll() {
            Ok(task) => {
                if let Some(res) = task.take_ready() {
                    match res {
                        Ok(creds) => {
                            return Action::LoginSuccess {
                                creds,
                                destination: self.destination.clone(),
                            }
                        }
                        Err(ServiceError::Unauthorized) => {
                            self.login_error = Some("Invalid username or password.".to_owned())
                        }
                        Err(error) => {
                            log::error!("login failed: {error}");
                            self.login_error = Some(error.to_string());
                        }
                    }
                }
            }
            Err(error) => {
                log::error!("login task failed: {error}");
                return Action::TaskFailed(TaskFailure::Stay {
                    message: "Login failed unexpectedly.".to_owned(),
                    retry: RetryAction::Login,
                });
            }
        }
        Action::None
    }

    pub fn is_busy(&self) -> bool {
        self.login_task.is_pending()
    }

    pub fn retry(&mut self, ctx: egui::Context) {
        self.start_login(ctx);
    }
}

fn show_login_header(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Image::new(egui::include_image!("../../assets/modama-tint.svg"))
                .tint(styles::color::on_surface::VALUE.resolve(ui.visuals()))
                .max_height(styles::typography::login_logo::FONT_SIZE),
        );
        ui.add_space(styles::spacing::header_title_gap::EGUI_POINTS);
        ui.label(
            RichText::new("Data Management")
                .font(styles::typography::section_heading::font_id())
                .color(styles::color::on_surface::VALUE.resolve(ui.visuals())),
        );
        ui.add_space(styles::spacing::header_subtitle_gap::EGUI_POINTS);
        ui.label(
            RichText::new("Ingest Agent")
                .font(styles::typography::supporting_text::font_id())
                .color(styles::color::on_surface_dim::VALUE.resolve(ui.visuals())),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_login_request_validation() {
        let req = LoginRequest {
            credentials: Credentials {
                username: "alex".to_string(),
                password: "pass".to_string(),
            },
            service: StubAgentService::shared(),
        };
        let res = req.execute();
        assert!(res.is_ok());
        assert_eq!(res.unwrap().username, "alex");
    }

    #[test]
    fn test_login_request_invalid_username() {
        let req = LoginRequest {
            credentials: Credentials {
                username: "stupid".to_string(),
                password: "pass".to_string(),
            },
            service: StubAgentService::shared(),
        };
        let res = req.execute();
        assert!(res.is_err());
    }

    #[test]
    fn operational_login_failures_remain_visible_inline() {
        let mut page = PageLogin::new();
        page.login_task = AsyncState::Ready {
            payload: Err(ServiceError::Transport("agent unavailable".to_owned())),
        };

        assert_eq!(page.poll(), Action::None);
        assert_eq!(
            page.login_error.as_deref(),
            Some("transport error: agent unavailable")
        );
    }

    #[test]
    fn login_failure_retains_the_destination_for_a_later_success() {
        let destination = LoginDestination::ConfirmClose {
            share_id: "share".to_owned(),
        };
        let mut page = PageLogin::with_service_and_destination(
            StubAgentService::shared(),
            destination.clone(),
        );
        page.login_task = AsyncState::Ready {
            payload: Err(ServiceError::Unauthorized),
        };

        assert_eq!(page.poll(), Action::None);

        let creds = Credentials {
            username: "user".to_owned(),
            password: "pass".to_owned(),
        };
        page.login_task = AsyncState::Ready {
            payload: Ok(creds.clone()),
        };
        assert_eq!(page.poll(), Action::LoginSuccess { creds, destination });
    }
}
