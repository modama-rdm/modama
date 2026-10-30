use std::{future::Future, path::Path, sync::Arc};

use anyhow::Result;
use log::info;
use tokio::{select, spawn, sync::Mutex};

use crate::{
    agent_state::AgentState, config::AgentConfig, local_http_service::serve_local_web_api_on,
    local_net_token_proxy::token_publisher_loop, message_loop::message_loop,
};

pub async fn run_agent<S, R>(config_path: &Path, shutdown: S, ready: R) -> Result<()>
where
    S: Future<Output = ()>,
    R: FnOnce() -> Result<()>,
{
    info!("looking for config at {config_path:?}");
    let config = AgentConfig::from_file(&config_path).await?;
    info!("This is modama-agent `{:?}`", config.identity);

    let state: Arc<Mutex<AgentState>> = AgentState::new(&config).await;
    let token_provider = config.make_token_provider().await?;

    let another_token_provider = Arc::clone(&token_provider);
    let _local_token_server_handle = config.client_cert.as_ref().map(|_client_cert| {
        spawn(async move {
            token_publisher_loop(another_token_provider).await;
        })
    });

    let listener = tokio::net::TcpListener::bind(&config.listen_local_api).await?;
    let state_for_api = Arc::clone(&state);
    let cloned_token_provider = Arc::clone(&token_provider);
    let local_api_handle = tokio::spawn(async move {
        serve_local_web_api_on(listener, state_for_api, cloned_token_provider).await
    });

    ready()?;
    tokio::pin!(shutdown);
    select! {
        _ = &mut shutdown => {
            info!("shutdown requested, exiting");
            Ok(())
        }
        _ = message_loop(
            Arc::clone(&state),
            Arc::clone(&config.client_config),
            Arc::clone(&token_provider),
        ) => Ok(()),
        res = local_api_handle => Ok(res??),
    }
}

#[cfg(unix)]
/// Wait for Ctrl-C (or SIGTERM on Unix)
pub async fn shutdown_signal() {
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
/// Wait for Ctrl-C (or SIGTERM on Unix)
pub async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        log::error!("could not wait for Ctrl-C: {error}");
    }
}
