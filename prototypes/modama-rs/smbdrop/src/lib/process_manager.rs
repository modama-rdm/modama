use std::{error::Error, process::Stdio, sync::Arc, time::Duration};

use log::{error, info, warn};
use nix::{
    sys::signal::{kill, Signal::SIGHUP},
    unistd::Pid,
};
use tokio::{
    process::{Child, Command},
    select,
    sync::{Mutex, mpsc},
};

pub trait ProcessManager: Clone + Sync + Send {
    async fn start_in_background(debug: bool) -> tokio::sync::mpsc::Sender<ProcessManagerCommand>;
}


#[derive(Clone)]
pub struct SambaProcessManager {
    child: Arc<Mutex<Child>>,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum ProcessManagerCommand {
    Reload,
    Stop,
}

#[derive(thiserror::Error, Debug)]
pub enum PmError {
    #[error("child is not running")]
    NotRunning,
}

#[derive(thiserror::Error, Debug)]
pub enum PmCommandOutcome {
    #[error("stopping")]
    Stopping,

    #[error("error while handling command")]
    Error(#[from] Box<dyn Error + Send + Sync>),
}

async fn handle_pm_command(
    mut pm: SambaProcessManager,
    cmd: &Option<ProcessManagerCommand>,
) -> Result<SambaProcessManager, PmCommandOutcome> {
    match cmd {
        None => {
            info!("other end has closed the control channel, exiting");
            pm.stop().await?;
            Err(PmCommandOutcome::Stopping)
        }
        Some(ProcessManagerCommand::Reload) => {
            info!("reloading child process...");
            pm.reload().await?;
            Ok(pm)
        }
        Some(ProcessManagerCommand::Stop) => {
            pm.stop().await?;
            Err(PmCommandOutcome::Stopping)
        }
    }
}

impl ProcessManager for SambaProcessManager {
    /// Start the samba process in the background and keep restarting.
    /// Reload or stop the process by sending commands to the channel that is returned.
    async fn start_in_background(debug: bool) -> mpsc::Sender<ProcessManagerCommand> {
        let (tx, mut rx) = mpsc::channel::<ProcessManagerCommand>(16);
        tokio::spawn(async move {
            'outer: loop {
                let mut pm = match Self::start(debug).await {
                    Ok(pm) => pm,
                    Err(e) => {
                        error!("failed to start subprocess ({e}), retrying...");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                };
                'inner: loop {
                    select! {
                        val = rx.recv() => {
                            pm = match handle_pm_command(pm, &val).await {
                                Err(PmCommandOutcome::Stopping) => {
                                    break 'outer;
                                }
                                Err(PmCommandOutcome::Error(e)) => {
                                    warn!("{e}; restarting...");
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                    continue 'outer;
                                }
                                Ok(p) => p,
                            };
                        }
                        _ = pm.wait() => {
                            warn!("child exited, restarting...");
                            break 'inner;
                        }
                    }
                }
            }
        });

        tx
    }

}

impl SambaProcessManager {
    pub async fn start(debug: bool) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut args = vec!["-F", "--debug-stdout", "--no-process-group"];
        if debug {
            args.push("-d3");
        }
        let child = Command::new("smbd")
            .args(&args)
            .stdin(Stdio::null())
            .spawn()?;

        Ok(Self { child: Arc::new(Mutex::new(child)) })
    }

    pub async fn stop(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.child.lock().await.kill().await?;
        Ok(())
    }

    pub async fn reload(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        if let Some(pid) = self.child.lock().await.id() {
            kill(Pid::from_raw(pid as i32), SIGHUP)?;
            Ok(())
        } else {
            Err(Box::new(PmError::NotRunning))
        }
    }

    pub async fn wait(&mut self) {
        // TODO: are there errors we can't ignore here?
        let _ = self.child.lock().await.wait().await;
    }
}
