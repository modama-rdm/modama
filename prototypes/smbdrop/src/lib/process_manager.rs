use std::{error::Error, process::Stdio, time::Duration};

use log::{error, info, warn};
use nix::{
    sys::signal::{kill, Signal::SIGHUP},
    unistd::Pid,
};
use tokio::{
    process::{Child, Command},
    select,
    sync::mpsc,
};

pub struct SambaProcessManager {
    child: Child,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum PmCommand {
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
    cmd: &Option<PmCommand>,
) -> Result<SambaProcessManager, PmCommandOutcome> {
    match cmd {
        None => {
            info!("other end has closed the control channel, exiting");
            pm.stop().await?;
            Err(PmCommandOutcome::Stopping)
        }
        Some(PmCommand::Reload) => {
            info!("reloading child process...");
            pm.reload().await?;
            Ok(pm)
        }
        Some(PmCommand::Stop) => {
            pm.stop().await?;
            Err(PmCommandOutcome::Stopping)
        }
    }
}

impl SambaProcessManager {
    /// Start the samba process in the background and keep restarting.
    /// Reload or stop the process by sending commands to the channel that is returned.
    pub async fn start_in_background() -> mpsc::Sender<PmCommand> {
        let (tx, mut rx) = mpsc::channel::<PmCommand>(16);
        tokio::spawn(async move {
            'outer: loop {
                let mut pm = match Self::start().await {
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

    pub async fn start() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let child = Command::new("smbd")
            .args(["-F", "--debug-stdout", "--no-process-group", "-d3"])
            .stdin(Stdio::null())
            .spawn()?;

        Ok(Self { child })
    }

    pub async fn stop(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.child.kill().await?;
        Ok(())
    }

    pub async fn reload(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        if let Some(pid) = self.child.id() {
            kill(Pid::from_raw(pid as i32), SIGHUP)?;
            Ok(())
        } else {
            Err(Box::new(PmError::NotRunning))
        }
    }

    pub async fn wait(&mut self) {
        // TODO: are there errors we can't ignore here?
        let _ = self.child.wait().await;
    }
}
