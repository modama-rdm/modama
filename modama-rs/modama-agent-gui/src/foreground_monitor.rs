use std::{
    sync::{Arc, mpsc},
    thread,
};

use eframe::egui;

use crate::{
    service::{AgentService, POLL_INTERVAL, ServiceError},
    state::ForegroundSession,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForegroundMonitorUpdate {
    ForegroundChanged(ForegroundSession),
    ConnectionDropped,
    ConnectionRestored,
}

pub struct ForegroundMonitor {
    updates: mpsc::Receiver<ForegroundMonitorUpdate>,
    stop: mpsc::Sender<()>,
}

fn poll_service(service: &dyn AgentService) -> Result<ForegroundSession, ServiceError> {
    service.health()?;
    service.foreground()
}

impl ForegroundMonitor {
    pub fn start(
        service: Arc<dyn AgentService>,
        initial: ForegroundSession,
        ctx: egui::Context,
    ) -> Self {
        let (update_tx, updates) = mpsc::channel();
        let (stop, stop_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut last = initial;
            let mut connection_dropped = false;
            loop {
                let mut updates = Vec::with_capacity(2);
                match poll_service(service.as_ref()) {
                    Ok(foreground) => {
                        if connection_dropped {
                            connection_dropped = false;
                            updates.push(ForegroundMonitorUpdate::ConnectionRestored);
                        }
                        if foreground != last {
                            last = foreground.clone();
                            updates.push(ForegroundMonitorUpdate::ForegroundChanged(foreground));
                        }
                    }
                    Err(error) => {
                        log::warn!("agent monitor poll failed: {error}");
                        if !connection_dropped {
                            connection_dropped = true;
                            updates.push(ForegroundMonitorUpdate::ConnectionDropped);
                        }
                    }
                }
                for update in updates {
                    if update_tx.send(update).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
                if stop_rx.recv_timeout(POLL_INTERVAL).is_ok() {
                    break;
                }
            }
        });
        Self { updates, stop }
    }

    pub fn try_recv(&self) -> Result<ForegroundMonitorUpdate, mpsc::TryRecvError> {
        self.updates.try_recv()
    }
}

impl Drop for ForegroundMonitor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Mutex, time::Duration};

    use crate::{
        service::{ConnectedAgent, ProjectSummary, ServiceError},
        state::{ActiveSession, Credentials, PendingSession},
    };

    use super::*;

    struct ForegroundOnlyService {
        health: Mutex<Result<(), ServiceError>>,
        foreground: Mutex<Result<ForegroundSession, ServiceError>>,
    }

    impl AgentService for ForegroundOnlyService {
        fn connect(&self) -> Result<ConnectedAgent, ServiceError> {
            unreachable!()
        }

        fn health(&self) -> Result<(), ServiceError> {
            self.health.lock().unwrap().clone()
        }

        fn foreground(&self) -> Result<ForegroundSession, ServiceError> {
            self.foreground.lock().unwrap().clone()
        }

        fn login(&self, _credentials: &Credentials) -> Result<(), ServiceError> {
            unreachable!()
        }

        fn list_projects(
            &self,
            _credentials: &Credentials,
        ) -> Result<Vec<ProjectSummary>, ServiceError> {
            unreachable!()
        }

        fn list_sessions(
            &self,
            _credentials: &Credentials,
            _project: &str,
        ) -> Result<Vec<String>, ServiceError> {
            unreachable!()
        }

        fn create_session(
            &self,
            _credentials: &Credentials,
            _project: &str,
            _session: &str,
        ) -> Result<(), ServiceError> {
            unreachable!()
        }

        fn start_ingest(
            &self,
            _credentials: &Credentials,
            _project: &str,
            _session: &str,
        ) -> Result<PendingSession, ServiceError> {
            unreachable!()
        }

        fn close_session(
            &self,
            _credentials: &Credentials,
            _session: &ActiveSession,
        ) -> Result<(), ServiceError> {
            unreachable!()
        }
    }

    fn recv_before_deadline(
        monitor: &ForegroundMonitor,
        timeout: Duration,
    ) -> ForegroundMonitorUpdate {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match monitor.try_recv() {
                Ok(update) => return update,
                Err(mpsc::TryRecvError::Empty) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                result => panic!("monitor did not forward state: {result:?}"),
            }
        }
    }

    #[test]
    fn monitor_forwards_changed_foreground_state() {
        let expected = ForegroundSession::Mounting(
            PendingSession {
                share_id: "share".to_owned(),
                project: "project".to_owned(),
                session: "session".to_owned(),
            },
            "Waiting for the session helper to mount the share.".to_owned(),
        );
        let service: Arc<dyn AgentService> = Arc::new(ForegroundOnlyService {
            health: Mutex::new(Ok(())),
            foreground: Mutex::new(Ok(expected.clone())),
        });
        let monitor =
            ForegroundMonitor::start(service, ForegroundSession::None, egui::Context::default());

        assert_eq!(
            recv_before_deadline(&monitor, Duration::from_secs(1)),
            ForegroundMonitorUpdate::ForegroundChanged(expected)
        );
    }

    #[test]
    fn monitor_reports_failure_once_and_recovers_without_foreground_change() {
        let service = Arc::new(ForegroundOnlyService {
            health: Mutex::new(Err(ServiceError::Server {
                status: 503,
                message: "not available".to_owned(),
            })),
            foreground: Mutex::new(Ok(ForegroundSession::None)),
        });
        let monitor = ForegroundMonitor::start(
            Arc::clone(&service) as Arc<dyn AgentService>,
            ForegroundSession::None,
            egui::Context::default(),
        );

        assert_eq!(
            recv_before_deadline(&monitor, Duration::from_secs(1)),
            ForegroundMonitorUpdate::ConnectionDropped
        );
        thread::sleep(POLL_INTERVAL + Duration::from_millis(50));
        assert_eq!(monitor.try_recv(), Err(mpsc::TryRecvError::Empty));

        *service.health.lock().unwrap() = Ok(());
        assert_eq!(
            recv_before_deadline(&monitor, Duration::from_secs(1)),
            ForegroundMonitorUpdate::ConnectionRestored
        );
    }

    #[test]
    fn foreground_error_makes_the_combined_poll_fail() {
        let service = ForegroundOnlyService {
            health: Mutex::new(Ok(())),
            foreground: Mutex::new(Err(ServiceError::Transport("dropped".to_owned()))),
        };

        assert_eq!(
            poll_service(&service),
            Err(ServiceError::Transport("dropped".to_owned()))
        );
    }
}
