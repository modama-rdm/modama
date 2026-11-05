use std::{future::Future, path::Path, sync::Arc};

use anyhow::Result;
use log::info;
use tokio::{select, spawn};

use crate::{
    config::AgentConfig, local_fan_out::LocalFanOut, local_http_service::serve_local_web_api_on,
    local_net_token_proxy::token_publisher_loop, message_loop::message_loop,
};

pub async fn run_agent<S, R>(config_path: &Path, shutdown: S, ready: R) -> Result<()>
where
    S: Future<Output = ()>,
    R: FnOnce() -> Result<()>,
{
    info!("looking for config at {config_path:?}");
    let config = Arc::new(AgentConfig::from_file(&config_path).await?);
    info!("This is modama-agent `{:?}`", config.identity);

    let fan_out = Arc::new(LocalFanOut::new(config.identity.clone()));
    let token_provider = config.make_token_provider().await?;

    let another_token_provider = Arc::clone(&token_provider);
    let _local_token_server_handle = config.client_cert.as_ref().map(|_client_cert| {
        spawn(async move {
            token_publisher_loop(another_token_provider).await;
        })
    });

    let listener = tokio::net::TcpListener::bind(&config.listen_local_api).await?;
    let config_for_api = Arc::clone(&config);
    let fan_out_for_api = Arc::clone(&fan_out);
    let cloned_token_provider = Arc::clone(&token_provider);
    let local_api_handle = tokio::spawn(async move {
        serve_local_web_api_on(
            listener,
            config_for_api,
            fan_out_for_api,
            cloned_token_provider,
        )
        .await
    });

    ready()?;
    tokio::pin!(shutdown);
    select! {
        _ = &mut shutdown => {
            info!("shutdown requested, exiting");
            Ok(())
        }
        _ = message_loop(
            Arc::clone(&config),
            Arc::clone(&fan_out),
            Arc::clone(&config.client_config),
            Arc::clone(&token_provider),
        ) => Ok(()),
        res = local_api_handle => Ok(res??),
    }
}
