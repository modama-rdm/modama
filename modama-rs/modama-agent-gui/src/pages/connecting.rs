use eframe::egui;
use std::time::Duration;

use crate::{
    actions::{Action, RetryAction, TaskFailure},
    async_state::AsyncState,
    state::AgentIdentity,
};

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
        if self.connect_task.is_idle() {
            let ctx = ui.ctx().clone();
            self.connect_task.spawn(
                move || {
                    crate::simulated_work::sleep(Duration::from_millis(1500));
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
        self.connect_task.is_idle() || self.connect_task.is_pending()
    }

    pub fn poll(&mut self) -> Action {
        match self.connect_task.poll() {
            Ok(task) => {
                if let Some(res) = task.take_ready() {
                    if res.is_ok() {
                        return Action::Connected {
                            identity: AgentIdentity::AgentId {
                                agent_name: "k2".to_owned(),
                                resource_name: "Holo".to_owned(),
                            },
                        };
                    }
                }
            }
            Err(error) => {
                log::error!("connecting task failed: {error}");
                return Action::TaskFailed(TaskFailure::Stay {
                    message: "Could not connect to the agent.".to_owned(),
                    retry: RetryAction::Reconnect,
                });
            }
        }
        Action::None
    }

    pub fn retry(&mut self, ctx: egui::Context) {
        self.connect_task.reset();
        ctx.request_repaint();
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
