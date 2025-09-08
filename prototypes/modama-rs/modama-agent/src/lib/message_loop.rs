use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result as AnyResult};

use crate::{
    agent_state::{AgentState, ConnectionState, ShareState},
    platform::{cleanup_mount_points, ensure_mounted, umount_old_shares},
};
use futures_util::{future::join_all, StreamExt};
use log::{debug, error, info, warn};
use modama_common::messages::{EventChannelMessage, Share};
use tokio::sync::Mutex;
use tokio_rustls::rustls::ClientConfig;
use tokio_tungstenite::connect_async_tls_with_config;

async fn update_share_list(
    state: &mut AgentState,
    desired_shares: ShareState<Share>,
) -> AnyResult<()> {
    let new_fg_share: Option<Share> = desired_shares.foreground_share;
    let old_fg_share: Option<&Share> = state
        .current_shares
        .foreground_share
        .as_ref()
        .map(|ms| &ms.share);
    if old_fg_share != new_fg_share.as_ref() {
        if let Some(share) = new_fg_share {
            state.current_shares.set_foreground_share(
                match ensure_mounted(&share, &state.config).await {
                    Ok(s) => Some(s),
                    Err(e) => {
                        error!("Mount failed: {e}");
                        None
                    }
                },
            );
        } else {
            state.current_shares.set_foreground_share(None);
        }
    }
    state.current_shares.set_shares(
        join_all(desired_shares.shares.into_iter().map(|s| async {
            let s2 = s;
            ensure_mounted(&s2, &state.config).await
        }))
        .await
        .into_iter()
        .flat_map(|res| {
            if let Err(err) = &res {
                warn!("Mounting failed: {err}");
            }
            res
        })
        .collect(),
    );

    debug!(
        "desired mounts: {:?}",
        &state
            .current_shares
            .shares
            .iter()
            .map(|s| s.mount_path.to_string_lossy().clone())
            .collect::<Vec<_>>()
    );

    umount_old_shares(&state.config, &state.current_shares.shares).await;
    cleanup_mount_points(&state.config);

    Ok(())
}

async fn handle_message(state: &Arc<Mutex<AgentState>>, msg: &str) -> AnyResult<()> {
    let event_msg: EventChannelMessage = serde_json::from_str(msg)
        .with_context(|| format!("Invalid message encountered: {}", msg))?;
    info!("Received EventChannelMessage: {event_msg:?}");

    match event_msg {
        EventChannelMessage::UpdateShareList {
            shares,
            foreground_share,
            connected_agents,
        } => {
            let desired_shares: ShareState<Share> = ShareState {
                foreground_share,
                shares: shares.into_iter().collect(),
            };
            state.lock().await.desired_shares = desired_shares.clone();
            state.lock().await.connected_agents = connected_agents;

            update_share_list(&mut *state.lock().await, desired_shares).await?;
            Ok(())
        }
    }
}

pub async fn message_loop(state: Arc<Mutex<AgentState>>, tls_config: Arc<ClientConfig>) {
    let config = state.lock().await.config.clone();

    loop {
        state.lock().await.connection = ConnectionState::Connecting;
        let (conn, _response) = match connect_async_tls_with_config(
            &config.api_url,
            None,
            false,
            Some(tokio_tungstenite::Connector::Rustls(Arc::clone(
                &tls_config,
            ))),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                let url = &config.api_url;
                error!("Could not connect to {url} ({e}) reconnecting...");
                error!("{}", e.to_string());
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        state.lock().await.connection = ConnectionState::Connected;

        let (mut _ws_write, mut ws_read) = conn.split();

        loop {
            match ws_read.next().await {
                Some(Ok(msg)) => {
                    if let Ok(msg) = msg.into_text() {
                        if let Err(e) = handle_message(&state, &msg).await {
                            error!("Error while handing message: {e}");
                        }
                    } else {
                        warn!("Message is not text, ignoring...");
                    }
                }
                Some(Err(e)) => {
                    error!("Could not receive message ({e}), reconnecting...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    break;
                }
                None => {
                    info!("Disconnected, trying to reconnect...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    break;
                }
            }
        }
    }
}
