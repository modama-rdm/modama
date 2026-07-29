use eframe::egui::{self, RichText, TextEdit};
use std::time::Duration;

use crate::{
    actions::Action, async_state::AsyncState, buttons::primary_button, state::Credentials, styles,
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
        let is_logging_in = self.login_task.is_pending();

        egui::Area::new(egui::Id::new("login-card"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.set_max_width(styles::spacing(12) * 10.0);
                let frame = styles::card_frame(
                    ui.visuals(),
                    egui::Margin::symmetric(
                        styles::token_to_px(styles::spacing::card_padding_x) as i8,
                        styles::token_to_px(styles::spacing::card_padding_y) as i8,
                    ),
                );
                frame.show(ui, |ui| {
                    ui.style_mut().spacing.item_spacing.y = 0.0;
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add(
                                egui::Image::new(egui::include_image!(
                                    "../../assets/modama-tint.svg"
                                ))
                                .tint(styles::color::fg.resolve_egui(ui.visuals()))
                                .max_height(
                                    styles::typography::login_logo
                                        .font_size
                                        .map(styles::token_to_px)
                                        .unwrap_or_default(),
                                ),
                            );
                            ui.add_space(styles::token_to_px(styles::spacing::header_title_gap));
                            ui.label(
                                RichText::new("Data Management")
                                    .font(styles::font::from_token(styles::typography::login_title))
                                    .color(styles::color::fg.resolve_egui(ui.visuals())),
                            );
                            ui.add_space(styles::token_to_px(styles::spacing::header_subtitle_gap));
                            ui.label(
                                RichText::new("Ingest Agent")
                                    .font(styles::font::from_token(
                                        styles::typography::login_subtitle,
                                    ))
                                    .color(styles::color::fg_subtle.resolve_egui(ui.visuals())),
                            );
                        });
                        ui.add_space(styles::token_to_px(styles::spacing::header_bottom_gap));

                        if let Some(error) = &self.login_error {
                            styles::danger_frame(ui.visuals()).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.colored_label(
                                    styles::color::danger.resolve_egui(ui.visuals()),
                                    RichText::new(error)
                                        .font(styles::font::from_token(styles::typography::body)),
                                );
                            });
                            ui.add_space(styles::spacing(7));
                        }

                        ui.add_enabled_ui(!is_logging_in, |ui| {
                            ui.style_mut().spacing.item_spacing.y = 0.0;

                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("Username")
                                        .font(styles::font::from_token(styles::typography::label))
                                        .color(styles::color::fg_muted.resolve_egui(ui.visuals())),
                                );
                                ui.add_space(styles::token_to_px(styles::spacing::field_label_gap));
                                let response = ui.add(
                                    TextEdit::singleline(&mut self.username)
                                        .font(styles::font::from_token(styles::typography::body))
                                        .frame(styles::input_frame(
                                            ui.visuals(),
                                            egui::Margin::symmetric(
                                                styles::token_to_px(
                                                    styles::spacing::input_padding_x,
                                                )
                                                    as i8,
                                                styles::token_to_px(
                                                    styles::spacing::input_padding_y,
                                                )
                                                    as i8,
                                            ),
                                        ))
                                        .desired_width(f32::INFINITY),
                                );
                                styles::paint_focus_ring(ui, &response);
                            });

                            ui.add_space(styles::token_to_px(styles::spacing::form_field_gap));

                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("Password")
                                        .font(styles::font::from_token(styles::typography::label))
                                        .color(styles::color::fg_muted.resolve_egui(ui.visuals())),
                                );
                                ui.add_space(styles::token_to_px(styles::spacing::field_label_gap));
                                let response = ui.add(
                                    TextEdit::singleline(&mut self.password)
                                        .font(styles::font::from_token(styles::typography::body))
                                        .password(true)
                                        .frame(styles::input_frame(
                                            ui.visuals(),
                                            egui::Margin::symmetric(
                                                styles::token_to_px(
                                                    styles::spacing::input_padding_x,
                                                )
                                                    as i8,
                                                styles::token_to_px(
                                                    styles::spacing::input_padding_y,
                                                )
                                                    as i8,
                                            ),
                                        ))
                                        .desired_width(f32::INFINITY),
                                );
                                styles::paint_focus_ring(ui, &response);
                            });
                        });

                        if is_logging_in {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Authenticating...")
                                        .font(styles::font::from_token(styles::typography::body)),
                                );
                            });
                        } else {
                            ui.add_space(styles::token_to_px(styles::spacing::form_field_gap));
                            ui.add_space(styles::token_to_px(styles::spacing::button_margin_top));
                            ui.vertical_centered(|ui| {
                                let login_btn = primary_button("Sign in", ui)
                                    .min_size(egui::vec2(ui.available_width(), 0.0));
                                let response = ui.add(login_btn);
                                styles::paint_focus_ring(ui, &response);
                                if response.clicked() {
                                    self.login_error = None;
                                    let req = LoginRequest {
                                        credentials: Credentials {
                                            username: self.username.clone(),
                                            password: self.password.clone(),
                                        },
                                    };
                                    let ctx = ui.ctx().clone();
                                    self.login_task.spawn(move || req.execute(), ctx);
                                }
                            });
                        }
                    });
                });
            });

        Action::None
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
