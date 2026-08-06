use std::{
    sync::{mpsc, Arc},
    thread,
};

use eframe::egui;

use crate::{
    service::{AgentService, POLL_INTERVAL},
    state::ForegroundSession,
};

pub struct ForegroundMonitor {
    updates: mpsc::Receiver<ForegroundSession>,
    stop: mpsc::Sender<()>,
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
            loop {
                match service.foreground() {
                    Ok(foreground) if foreground != last => {
                        last = foreground.clone();
                        if update_tx.send(foreground).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                    Ok(_) => {}
                    Err(error) => log::warn!("foreground-state poll failed: {error}"),
                }
                if stop_rx.recv_timeout(POLL_INTERVAL).is_ok() {
                    break;
                }
            }
        });
        Self { updates, stop }
    }

    pub fn try_recv(&self) -> Result<ForegroundSession, mpsc::TryRecvError> {
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
        foreground: Mutex<ForegroundSession>,
    }

    impl AgentService for ForegroundOnlyService {
        fn connect(&self) -> Result<ConnectedAgent, ServiceError> {
            unreachable!()
        }

        fn foreground(&self) -> Result<ForegroundSession, ServiceError> {
            Ok(self.foreground.lock().unwrap().clone())
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

    #[test]
    fn monitor_forwards_changed_foreground_state() {
        let expected = ForegroundSession::Mounting(PendingSession {
            share_id: "share".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
        });
        let service: Arc<dyn AgentService> = Arc::new(ForegroundOnlyService {
            foreground: Mutex::new(expected.clone()),
        });
        let monitor =
            ForegroundMonitor::start(service, ForegroundSession::None, egui::Context::default());

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            match monitor.try_recv() {
                Ok(update) => {
                    assert_eq!(update, expected);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                result => panic!("monitor did not forward state: {result:?}"),
            }
        }
    }
}
