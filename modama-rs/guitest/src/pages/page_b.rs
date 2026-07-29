use std::time::Duration;

use eframe::egui;

use crate::{actions::Action, async_state::AsyncState};

#[derive(Debug, Clone, PartialEq)]
pub struct NameCheckRequest {
    pub name: String,
    pub threshold: f32,
}

impl NameCheckRequest {
    pub fn new(name: String) -> Self {
        Self {
            name,
            threshold: 0.9,
        }
    }

    pub fn execute(&self) -> NameCheckResponse {
        std::thread::sleep(Duration::from_millis(500));
        let r = rand::random_range(0.0..1.0f32);
        self.evaluate(r)
    }

    pub fn evaluate(&self, random_val: f32) -> NameCheckResponse {
        NameCheckResponse {
            name: self.name.clone(),
            name_is_good: random_val > self.threshold,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameCheckResponse {
    pub name: String,
    pub name_is_good: bool,
}

impl NameCheckResponse {
    pub fn into_action(self) -> Action {
        if self.name_is_good {
            Action::ConfirmName { name: self.name }
        } else {
            Action::None
        }
    }
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
                let request = NameCheckRequest::new(self.new_name.clone());
                self.name_checking.spawn(
                    move || request.execute(),
                    ui.ctx().clone(),
                );
            }
            Action::None
        })
        .inner
    }

    pub fn poll(&mut self) -> Action {
        if let Some(payload) = self.name_checking.poll().take_ready() {
            return payload.into_action();
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_check_request_evaluate_success() {
        let req = NameCheckRequest::new("Alice".to_string());
        let res = req.evaluate(0.95);
        assert!(res.name_is_good);
        assert_eq!(res.name, "Alice");
        assert_eq!(res.into_action(), Action::ConfirmName { name: "Alice".to_string() });
    }

    #[test]
    fn test_name_check_request_evaluate_failure() {
        let req = NameCheckRequest::new("Bob".to_string());
        let res = req.evaluate(0.5);
        assert!(!res.name_is_good);
        assert_eq!(res.into_action(), Action::None);
    }

    #[test]
    fn test_custom_threshold_evaluation() {
        let req = NameCheckRequest {
            name: "Charlie".to_string(),
            threshold: 0.0,
        };
        let res = req.evaluate(0.01);
        assert!(res.name_is_good);
    }
}
