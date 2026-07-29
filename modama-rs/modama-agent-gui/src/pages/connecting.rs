use std::time::Duration;
use eframe::egui;

use crate::{actions::Action, async_state::AsyncState};

pub struct PageConnecting {
    connect_task: AsyncState<Result<(), String>>,
}

impl Default for PageConnecting {
    fn default() -> Self {
        Self::new()
    }
}

impl PageConnecting {
    pub fn new() -> Self {
        Self {
            connect_task: AsyncState::Idle,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        if matches!(self.connect_task, AsyncState::Idle) {
            let ctx = ui.ctx().clone();
            self.connect_task.spawn(
                move || {
                    std::thread::sleep(Duration::from_millis(500));
                    Ok(())
                },
                ctx,
            );
        }

        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.label("Connecting to modama...");
        });

        Action::None
    }

    pub fn poll(&mut self) -> Action {
        if let Some(res) = self.connect_task.poll().take_ready() {
            if res.is_ok() {
                return Action::GoToOverview;
            }
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connecting_init() {
        let page = PageConnecting::new();
        assert!(!page.connect_task.is_pending());
    }
}
