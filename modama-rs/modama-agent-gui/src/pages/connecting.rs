use eframe::egui;
use std::time::Duration;

use crate::{actions::Action, async_state::AsyncState};

pub struct PageConnecting {
    pub connect_task: AsyncState<Result<(), String>>,
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
                    std::thread::sleep(Duration::from_millis(1500));
                    Ok(())
                },
                ctx,
            );
        }

        egui::Area::new(egui::Id::new("connecting-status"))
            .fixed_pos(ui.max_rect().center())
            .pivot(egui::Align2::CENTER_CENTER)
            .show(ui.ctx(), |ui| {
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.label("Connecting to modama...");
                });
            });

        Action::None
    }

    pub fn is_busy(&self) -> bool {
        matches!(
            self.connect_task,
            AsyncState::Idle | AsyncState::Pending { .. }
        )
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
        assert!(page.is_busy());
    }
}
