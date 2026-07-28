use std::time::Duration;

use eframe::egui;

use crate::{actions::Action, async_state::AsyncState};

#[derive(Debug, Clone)]
struct NameCheckResponse {
    name: String,
    name_is_good: bool,
}

pub struct StateB {
    new_name: String,
    name_checking: AsyncState<NameCheckResponse>,
}

impl Default for StateB {
    fn default() -> Self {
        Self::new()
    }
}

impl StateB {
    pub fn new() -> Self {
        Self {
            new_name: "".to_owned(),
            name_checking: AsyncState::Idle,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        let busy = matches!(self.name_checking, AsyncState::Pending { .. });
        ui.add_enabled_ui(!busy, |ui| {
            ui.text_edit_singleline(&mut self.new_name);
            if ui.button("Next").clicked() {
                let name_to_check = self.new_name.clone();
                self.name_checking.spawn(
                    move || {
                        std::thread::sleep(Duration::from_millis(500));
                        let r = rand::random_range(0.0..1.0f32);
                        NameCheckResponse {
                            name: name_to_check,
                            name_is_good: r > 0.9,
                        }
                    },
                    ui.ctx().clone(),
                );
                Action::None
            } else {
                Action::None
            }
        })
        .inner
    }

    pub fn poll(&mut self) -> Action {
        if let AsyncState::Ready { payload } = self.name_checking.poll() {
            if payload.name_is_good {
                return Action::ConfirmName {
                    name: payload.name.clone(),
                };
            }
        }
        Action::None
    }
}
