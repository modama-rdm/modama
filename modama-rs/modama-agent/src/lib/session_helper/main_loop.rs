use std::{ops::ControlFlow, sync::Arc, time::Duration};

use anyhow::Context as _;
use futures_util::StreamExt as _;
use log::{error, info, warn};
use modama_common::internal_api_types::ToSessionHelper;
use rand::RngExt;
use tokio::{sync::Mutex, time::Instant};
use tokio_tungstenite::connect_async;
use tungstenite::Message;

use crate::session_helper::{
    helper_state::HelperState, mount_backend::MountBackend, reconcile::reconcile,
};

async fn handle_message(
    backend: &impl MountBackend,
    state: &Arc<Mutex<HelperState>>,
    msg: &str,
) -> AnyResult<()> {
    let event_msg: ToSessionHelper = serde_json::from_str(msg)
        .with_context(|| format!("Invalid message encountered: {}", msg))?;
    info!("Received EventChannelMessage: {event_msg:?}");

    state.lock().await.desired_state = event_msg.share_state.clone();
    if let Some(desired_shares) = event_msg.share_state {
        backend
            .reconcile(&desired_shares.shares, &event_msg.config)
            .await;
    }
}

async fn receive_inner(
    message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    backend: &impl MountBackend,
    state: &Arc<Mutex<HelperState>>,
) -> ControlFlow<(), ()> {
    match message {
        Some(Ok(Message::Text(utf8_bytes))) => {
            if let Err(e) = handle_message(backend, state, &utf8_bytes.as_str()).await {
                error!("Error while handing message: {e}");
            }
            ControlFlow::Continue(())
        }
        Some(Ok(_)) => {
            warn!("Message is not text, ignoring...");
            ControlFlow::Continue(())
        }
        Some(Err(e)) => {
            error!("Could not receive message ({e}), reconnecting...");
            tokio::time::sleep(Duration::from_secs(5)).await;
            ControlFlow::Break(())
        }
        None => {
            info!("Disconnected, trying to reconnect...");
            tokio::time::sleep(Duration::from_secs(5)).await;
            ControlFlow::Break(())
        }
    }
}

pub async fn session_helper_loop(state: Arc<Mutex<HelperState>>, backend: &impl MountBackend) {
    // [ ] connect to the internal websocket API
    // receive messages
    // [x] add a timeout when receiving, so we reconcile periodically
    // -> we need to keep the latest received desired state around for periodic reconciliation
    // [ ] handle each message by calling the reconciliation function(s)
    //
    //

    loop {
        state.lock().await.connection_state =
            crate::session_helper::helper_state::HelperConnectionState::Connecting;

        // FIXME: configurable base URL
        let (conn, _resp) = connect_async("http://localhost:3003/api/internal/events/")
            .await
            .unwrap();
        let (mut ws_write, mut ws_read) = conn.split();

        state.lock().await.connection_state =
            crate::session_helper::helper_state::HelperConnectionState::Connected;

        let reconcile_interval =
            Duration::from_millis(1000u64 + rand::rng().random_range(0..500u64));
        let timer = tokio::time::sleep(reconcile_interval);
        tokio::pin!(timer);
        loop {
            // keep this outside the select! so the reconciliation
            // can finish even if a websocket message arrives concurrently:
            if let Some(config) = &state.lock().await.config {
                reconcile(backend, &config, &state).await;
            }
            tokio::select! {
                _ = &mut timer => {
                    timer.as_mut().reset(
                        Instant::now() + reconcile_interval + Duration::from_millis(rand::rng().random_range(0..300u64))
                    )
                }
                msg = ws_read.next() => {
                    match receive_inner(msg, backend, &state).await {
                        ControlFlow::Continue(_) => continue,  // success or ignored message
                        ControlFlow::Break(_) => break,  // error or disconnected => reconnect
                    }
                }
            }
        }
    }
}
