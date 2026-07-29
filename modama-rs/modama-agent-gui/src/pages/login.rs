use eframe::egui::{self, RichText, TextEdit};
use std::time::Duration;

use crate::{
    actions::Action,
    async_state::AsyncState,
    buttons::{primary_button, primary_loading_button},
    components::{display_error, labeled_text_input},
    state::Credentials,
    styles,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginRequest {
    pub credentials: Credentials,
}

impl LoginRequest {
    pub fn execute(&self) -> Result<Credentials, String> {
        std::thread::sleep(Duration::from_millis(500));
        if self.credentials.username.trim().is_empty() {
            Err("Username cannot be empty".to_string())
        } else {
            Ok(self.credentials.clone())
        }
    }
}

pub struct PageLogin {
    pub username: String,
    pub password: String,
    pub login_error: Option<String>,
    pub login_task: AsyncState<Result<Credentials, String>>,
}

impl Default for PageLogin {
    fn default() -> Self {
        Self::new()
    }
}

impl PageLogin {
    pub fn new() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            login_error: None,
            login_task: AsyncState::Idle,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        egui::Area::new(egui::Id::new("login-card"))
            .fixed_pos(ui.max_rect().center())
            .pivot(egui::Align2::CENTER_CENTER)
            .show(ui.ctx(), |ui| self.show_login_card(ui));

        Action::None
    }

    fn show_login_card(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(styles::components::login_card::width::EGUI_POINTS);
        styles::card_frame(ui.visuals()).show(ui, |ui| {
            ui.style_mut().spacing.item_spacing.y = 0.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                self.show_card_content(ui);
            });
        });
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
                let user_lost_focus =
                    labeled_text_input(ui, "Username", TextEdit::singleline(&mut self.username))
                        .lost_focus();
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
        };
        self.login_task.spawn(move || req.execute(), ctx);
    }

    pub fn poll(&mut self) -> Action {
        if let Some(res) = self.login_task.poll().take_ready() {
            match res {
                Ok(creds) => return Action::LoginSuccess { creds },
                Err(_err) => {
                    self.login_error = Some(_err);
                }
            }
        }
        Action::None
    }

    pub fn is_busy(&self) -> bool {
        self.login_task.is_pending()
    }
}

fn show_login_header(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Image::new(egui::include_image!("../../assets/modama-tint.svg"))
                .tint(styles::color::on_surface::VALUE.resolve_egui(ui.visuals()))
                .max_height(styles::typography::login_logo::FONT_SIZE),
        );
        ui.add_space(styles::spacing::header_title_gap::EGUI_POINTS);
        ui.label(
            RichText::new("Data Management")
                .font(styles::typography::section_heading::font_id())
                .color(styles::color::on_surface::VALUE.resolve_egui(ui.visuals())),
        );
        ui.add_space(styles::spacing::header_subtitle_gap::EGUI_POINTS);
        ui.label(
            RichText::new("Ingest Agent")
                .font(styles::typography::supporting_text::font_id())
                .color(styles::color::on_surface_dim::VALUE.resolve_egui(ui.visuals())),
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
        };
        let res = req.execute();
        assert!(res.is_ok());
        assert_eq!(res.unwrap().username, "alex");
    }

    #[test]
    fn test_login_request_empty_username() {
        let req = LoginRequest {
            credentials: Credentials {
                username: "   ".to_string(),
                password: "pass".to_string(),
            },
        };
        let res = req.execute();
        assert!(res.is_err());
    }
}
