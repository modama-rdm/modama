use std::time::Duration;
use eframe::egui::{self, Label, RichText, TextEdit};

use crate::{
    actions::Action,
    async_state::AsyncState,
    buttons::action_button,
    state::Credentials,
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
            login_task: AsyncState::Idle,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        let is_logging_in = self.login_task.is_pending();

        ui.add(Label::new(RichText::new("Login").size(25.0).strong()));

        ui.add_enabled_ui(!is_logging_in, |ui| {
            ui.horizontal(|ui| {
                ui.label("Username:");
                ui.text_edit_singleline(&mut self.username);
            });
            ui.horizontal(|ui| {
                ui.label("Password:");
                ui.add(TextEdit::singleline(&mut self.password).password(true));
            });
        });

        if is_logging_in {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Authenticating...");
            });
        } else {
            let login_btn = action_button("Login");
            if ui.add(login_btn).clicked() {
                let req = LoginRequest {
                    credentials: Credentials {
                        username: self.username.clone(),
                        password: self.password.clone(),
                    },
                };
                let ctx = ui.ctx().clone();
                self.login_task.spawn(move || req.execute(), ctx);
            }
        }

        Action::None
    }

    pub fn poll(&mut self) -> Action {
        if let Some(res) = self.login_task.poll().take_ready() {
            match res {
                Ok(creds) => return Action::LoginSuccess { creds },
                Err(_err) => {
                    // Stay on login page on failure
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
