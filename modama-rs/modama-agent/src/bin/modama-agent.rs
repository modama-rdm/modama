use std::{path::PathBuf, sync::Arc};

use anyhow::{Result as AnyResult, anyhow};
use clap::Parser;
use log::info;
use modama_agent::{
    agent_state::AgentState,
    config::{AgentConfig, CliArgs},
    local_http_service::serve_local_web_api,
    local_net_token_proxy::token_publisher_loop,
    message_loop::message_loop,
    mount_backend::MountBackend,
    platform::CurrentPlatformMountBackend,
};
use modama_common::logging::init_logging;
use tokio::{select, signal::unix::SignalKind, spawn, sync::Mutex};

fn config_path(args: &CliArgs) -> AnyResult<PathBuf> {
    Ok(match &args.config_path {
        Some(path) => path.clone(),
        None => {
            // FIXME: add more paths where we search for config? /etc/modama-agent.toml?
            let user_local_config =
                directories::ProjectDirs::from("de", "fz-juelich", "modama-agent")
                    .map(|p| p.config_dir().join("modama-agent.toml"))
                    .ok_or(anyhow!("could not find project directories for configuration"))?;
            user_local_config
        }
    })
}

#[tokio::main]
async fn main() -> AnyResult<()> {
    init_logging();

    let args = CliArgs::parse();
    let path = config_path(&args)?;
    info!("looking for config at {path:?}");
    let config = AgentConfig::from_file(&path).await?;

    info!("This is modama-agent `{:?}`", &config.identity);

    let state: Arc<Mutex<AgentState>> = Arc::new(Mutex::new(AgentState::new(&config)));

    let token_provider = config.make_token_provider().await?;

    let another_token_provider = Arc::clone(&token_provider);
    let _local_token_server_handle = config.client_cert.as_ref().map(|_client_cert| {
        spawn(async move {
            token_publisher_loop(another_token_provider).await;
        })
    });

    let state_for_api = Arc::clone(&state);
    let cloned_token_provider = Arc::clone(&token_provider);
    let listen_local_api = config.listen_local_api.clone();
    let local_api_handle = tokio::spawn(async move {
        serve_local_web_api(state_for_api, &listen_local_api, cloned_token_provider).await
    });

    let mount_backend = CurrentPlatformMountBackend {};
    mount_backend.initialize(&config).await;

    #[cfg(unix)]
    let mut term_stream = tokio::signal::unix::signal(SignalKind::terminate())?;

    select! {
        _ = tokio::signal::ctrl_c() => {
            eprintln!("SIGINT received, exiting");
            Ok(())
        },
        _ = term_stream.recv() => {
            eprintln!("SIGTERM received, exiting");
            Ok(())
        }
        _ = message_loop(&mount_backend, Arc::clone(&state), Arc::clone(&config.client_config), Arc::clone(&token_provider)) => {
            Ok(())
        }
        res = local_api_handle => {
            res?
        }
    }?;

    Ok(())
}
