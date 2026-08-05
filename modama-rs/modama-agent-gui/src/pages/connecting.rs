use eframe::egui;
use std::{fmt::Display, sync::Arc, time::Duration};

use crate::{
    actions::Action,
    async_state::AsyncState,
    service::{AgentService, ConnectedAgent, StubAgentService},
};

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(16);

pub struct PageConnecting {
    pub connect_task: AsyncState<ConnectedAgent>,
    service: Arc<dyn AgentService>,
}

fn connect_with_retry<T, E>(
    mut connect: impl FnMut() -> Result<T, E>,
    mut sleep: impl FnMut(Duration),
) -> T
where
    E: Display,
{
    let mut retry_delay = INITIAL_RETRY_DELAY;
    loop {
        match connect() {
            Ok(connected) => return connected,
            Err(error) => {
                log::error!(
                    "connecting to the agent failed: {error}; retrying in {} seconds",
                    retry_delay.as_secs()
                );
                sleep(retry_delay);
                retry_delay = retry_delay.saturating_mul(2).min(MAX_RETRY_DELAY);
            }
        }
    }
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
            self.connect_task.spawn(
                move || connect_with_retry(|| service.connect(), std::thread::sleep),
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
                if let Some(connected) = task.take_ready() {
                    return Action::Connected {
                        identity: connected.identity,
                        session: connected.session,
                    };
                }
            }
            Err(error) => {
                log::error!("connecting task failed: {error}");
                self.connect_task.reset();
            }
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn test_connecting_init() {
        let page = PageConnecting::new();
        assert!(!page.connect_task.is_pending());
        assert!(page.is_busy());
    }

    #[test]
    fn connection_succeeds_without_delay_on_the_first_attempt() {
        let mut delays = Vec::new();

        let result = connect_with_retry(|| Ok::<_, &str>("connected"), |delay| delays.push(delay));

        assert_eq!(result, "connected");
        assert!(delays.is_empty());
    }

    #[test]
    fn connection_retries_with_capped_exponential_backoff() {
        let attempts = Cell::new(0_u8);
        let mut delays = Vec::new();

        let result = connect_with_retry(
            || {
                let attempt = attempts.get();
                attempts.set(attempt + 1);
                if attempt < 6 {
                    Err("not ready")
                } else {
                    Ok("connected")
                }
            },
            |delay| delays.push(delay),
        );

        assert_eq!(result, "connected");
        assert_eq!(delays, [1, 2, 4, 8, 16, 16].map(Duration::from_secs));
    }

    #[test]
    fn worker_failure_resets_for_an_automatic_restart() {
        let mut page = PageConnecting::new();
        page.connect_task.spawn(
            || -> ConnectedAgent { panic!("worker failure") },
            egui::Context::default(),
        );

        while page.connect_task.is_pending() {
            assert_eq!(page.poll(), Action::None);
            std::thread::yield_now();
        }

        assert!(page.connect_task.is_idle());
        assert_eq!(page.poll(), Action::None);
    }
}
