use eframe::egui;
use std::sync::Arc;

use crate::{
    actions::{Action, RetryAction, TaskFailure},
    async_state::AsyncState,
    service::{AgentService, ConnectedAgent, ServiceError, StubAgentService},
};

pub struct PageConnecting {
    pub connect_task: AsyncState<Result<ConnectedAgent, ServiceError>>,
    service: Arc<dyn AgentService>,
}

impl Default for PageConnecting {
    fn default() -> Self {
        Self::new()
    }
}

impl PageConnecting {
    pub fn new() -> Self {
        Self::with_service(StubAgentService::shared())
    }

    pub fn with_service(service: Arc<dyn AgentService>) -> Self {
        Self {
            connect_task: AsyncState::Idle,
            service,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Action {
        if self.connect_task.is_idle() {
            let ctx = ui.ctx().clone();
            let service = Arc::clone(&self.service);
            self.connect_task.spawn(move || service.connect(), ctx);
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
                    match res {
                        Ok(connected) => {
                            return Action::Connected {
                                identity: connected.identity,
                                session: connected.session,
                            }
                        }
                        Err(error) => {
                            log::error!("connecting failed: {error}");
                            return Action::TaskFailed(TaskFailure::Stay {
                                message: "Could not connect to the agent.".to_owned(),
                                retry: RetryAction::Reconnect,
                            });
                        }
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
