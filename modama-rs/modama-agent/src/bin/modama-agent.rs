use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use modama_agent::{
    config::{CliArgs, Command},
    message_loop::MountPolicy,
    runtime::run_agent,
};
use modama_common::logging::init_logging;

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::SignalKind;

    let terminate = async {
        match tokio::signal::unix::signal(SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                log::error!("could not register SIGTERM handler: {error}");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(error) = result {
                log::error!("could not wait for Ctrl-C: {error}");
            }
        }
        _ = terminate => {}
    }
}

#[cfg(windows)]
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        log::error!("could not wait for Ctrl-C: {error}");
    }
}

#[cfg(not(windows))]
fn default_config_path() -> Result<PathBuf> {
    directories::ProjectDirs::from("de", "fz-juelich", "modama-agent")
        .map(|paths| paths.config_dir().join("modama-agent.toml"))
        .ok_or_else(|| anyhow::anyhow!("could not find project directories for configuration"))
}

#[cfg(windows)]
fn default_config_path() -> Result<PathBuf> {
    modama_agent::windows_service::user_config_path()
}

fn config_path(args: &CliArgs) -> Result<PathBuf> {
    args.config_path
        .clone()
        .map_or_else(default_config_path, Ok)
}

fn run_foreground(path: PathBuf) -> Result<()> {
    init_logging();
    tokio::runtime::Runtime::new()?.block_on(run_agent(
        &path,
        MountPolicy::Enabled,
        shutdown_signal(),
        || Ok(()),
    ))
}

fn main() -> Result<()> {
    let args = CliArgs::parse();
    match args.command {
        None => run_foreground(config_path(&args)?),
        Some(Command::RunService) => {
            let path = config_path(&args)?;
            #[cfg(windows)]
            {
                modama_agent::windows_service::run_service_dispatcher(path)
            }
            #[cfg(not(windows))]
            {
                run_foreground(path)
            }
        }
        #[cfg(windows)]
        Some(Command::InstallAsService) => {
            let source =
                modama_agent::windows_service::resolve_install_config(args.config_path.as_deref())?;
            modama_agent::windows_service::install(&source)
        }
        #[cfg(windows)]
        Some(Command::UninstallService) => modama_agent::windows_service::uninstall(),
        #[cfg(windows)]
        Some(Command::StartService) => modama_agent::windows_service::start(),
        #[cfg(windows)]
        Some(Command::StopService) => modama_agent::windows_service::stop(),
        #[cfg(windows)]
        Some(Command::ServiceStatus) => modama_agent::windows_service::status(),
    }
}
