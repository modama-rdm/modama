use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use futures_util::{future::join_all, StreamExt};
use log::{error, info, warn};
use modama_agent::{
    common::init_logging,
    config::{AgentConfig, AgentLinuxConfig},
    messages::{CurrentForegroundShare, EventChannelMessage, ShareMsg},
    platform::{cleanup_mount_points, ensure_mounted, umount_old_shares},
    share::{MountedShare, Share},
};
use tokio::sync::{Mutex, OnceCell};
use tokio_tungstenite::connect_async;
use url::Url;

#[derive(Debug, Default)]
enum ConnectionState {
    #[default]
    Connecting,
    Connected,
}

#[derive(Debug)]
struct AgentState {
    connection: ConnectionState,

    /// The current foreground share, if any.
    foreground_share: Option<MountedShare>,

    /// All shares, including the foreground share
    shares: Vec<MountedShare>,

    /// Runtime configuration
    config: AgentConfig,
}

impl AgentState {
    fn new(config: &AgentConfig) -> Self {
        let config = config.clone();
        Self {
            connection: Default::default(),
            foreground_share: None,
            shares: Vec::new(),
            config,
        }
    }

    fn set_foreground_share(&mut self, share: Option<MountedShare>) {
        self.foreground_share = share;
        self.ensure_consistent();
    }

    fn ensure_consistent(&mut self) {
        // ensure the foreground share is part of the list of all shares:
        if let Some(share) = &self.foreground_share {
            if !self.shares.contains(share) {
                self.shares.push(share.clone());
            }
        }
    }

    fn set_shares(&mut self, shares: Vec<MountedShare>) {
        self.shares = shares;
        self.ensure_consistent();
    }
}

/// Return whether the two paths are considered equal.
///
/// If the one of the paths doesn't canonicalize, either the path doesn't exist
/// or is straight up invalid (both cases of TOCTOU?) -> log error and fall back
/// to strict equality.
async fn update_share_list(
    state: &mut AgentState,
    shares: Vec<ShareMsg>,
    foreground_share: Option<ShareMsg>,
) -> Result<()> {
    let new_fg_share: Option<Share> = foreground_share.map(Into::into);
    let old_fg_share: Option<&Share> = state.foreground_share.as_ref().map(|ms| &ms.share);
    if old_fg_share != new_fg_share.as_ref() {
        if let Some(share) = new_fg_share {
            state.set_foreground_share(match ensure_mounted(&share, &state.config).await {
                Ok(s) => Some(s),
                Err(e) => {
                    error!("Mount failed: {e}");
                    None
                }
            });
        } else {
            state.set_foreground_share(None);
        }
    }
    state.set_shares(
        join_all(
            shares
                .into_iter()
                .map(|s| async { ensure_mounted(&s.into(), &state.config).await }),
        )
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

    umount_old_shares(&state.config, &state.shares).await;
    cleanup_mount_points(&state.config);

    Ok(())
}

async fn handle_message(state: &Arc<Mutex<AgentState>>, msg: &str) -> Result<()> {
    let event_msg: EventChannelMessage =
        serde_json::from_str(msg).context("Invalid message encountered")?;
    match event_msg {
        EventChannelMessage::UpdateShareList { .. } => {
            info!("Received EventChannelMessage::UpdateShareList");
        }
    }

    match event_msg {
        EventChannelMessage::UpdateShareList {
            shares,
            foreground_share,
        } => {
            update_share_list(&mut *state.lock().await, shares, foreground_share).await?;
            Ok(())
        }
    }
}

async fn message_loop(state: Arc<Mutex<AgentState>>) {
    let config = state.lock().await.config.clone();

    loop {
        state.lock().await.connection = ConnectionState::Connecting;
        let (conn, _response) = match connect_async(&config.api_url).await {
            Ok(s) => s,
            Err(e) => {
                let url = &config.api_url;
                error!("Could not connect to {url} ({e}) reconnecting...");
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

/// Wrapper around the AgentState for the local API server
#[derive(Clone)]
struct ApiState {
    pub state: Arc<Mutex<AgentState>>,
}

impl ApiState {
    fn new(state: &Arc<Mutex<AgentState>>) -> Self {
        Self {
            state: Arc::clone(state),
        }
    }
}

// #[debug_handler]
async fn handler_shares_foreground(
    State(api_state): State<ApiState>,
) -> Json<CurrentForegroundShare> {
    Json(match &api_state.state.lock().await.foreground_share {
        None => CurrentForegroundShare::None,
        Some(s) => match s.mount_path.to_str() {
            None => {
                let p = s.mount_path.to_string_lossy();
                warn!("Mount path {p} contains invalid unicode, cannot get foreground share!");
                CurrentForegroundShare::None
            }
            Some(mount_path) => CurrentForegroundShare::Share {
                dest_path: s.share.dest_path.to_owned(),
                mount_path: mount_path.to_owned(),
            },
        },
    })
}

async fn serve_local_web_api(state: Arc<Mutex<AgentState>>) {
    let app = Router::new()
        .route("/shares/foreground/", get(handler_shares_foreground))
        .with_state(ApiState::new(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3003")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[tokio::main]
async fn main() {
    init_logging();

    let smb_hostname = "moellenstedt.iff.kfa-juelich.de".to_string();

    let config = &AgentConfig {
        api_url: Url::parse("ws://localhost:1234").expect("invalid websocket URL"),
        smb_hostname,
        smb_domain: "ER-C-INGEST".to_string(),
        smb_ip: OnceCell::new(),
        agent_name: "test".to_string(),
        resource_name: "test".to_string(),
        linux_config: Some(AgentLinuxConfig::default()),
    };

    info!(
        "This is modama-agent `{}` running for resource `{}`",
        &config.agent_name, config.resource_name
    );

    let state: Arc<Mutex<AgentState>> = Arc::new(Mutex::new(AgentState::new(config)));

    tokio::spawn(serve_local_web_api(Arc::clone(&state)));
    message_loop(Arc::clone(&state)).await;
}
