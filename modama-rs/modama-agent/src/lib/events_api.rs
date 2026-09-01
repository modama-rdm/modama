use std::{ops::DerefMut as _, path::PathBuf, sync::Arc};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::header::HeaderMap,
    response::{IntoResponse as _, Response},
};
use hyper::StatusCode;
use log::{error, warn};
use modama_common::messages::Share;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    select,
    sync::{Mutex, broadcast, mpsc},
};

use crate::{
    agent_state::{AgentState, StateUpdateToSession},
    local_http_service::ApiState,
    share::MountedShare,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionId {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHelperConfig {
    smb_hostname: String,
    smb_port: u32,
}

#[derive(thiserror::Error, Debug)]
pub enum SessionError {
    #[error("could not determine session identity")]
    CouldNotId(#[from] std::env::VarError),
}

impl SessionId {
    pub fn from_env() -> Result<Self, SessionError> {
        #[cfg(target_os = "linux")]
        let environment = std::env::var("XDG_SESSION_ID")?;
        #[cfg(windows)]
        let environment = std::env::var("SESSIONNAME")?;

        Ok(Self { id: environment })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionShare {
    share: Share,
    path: PathBuf,
}

/// Messages sent over the internal websocket connection
/// from either modama-agent-gui or the session helper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InternalMsgToAgent {
    /// This must be the first message from a GUI connection
    HelloGui { session_id: SessionId },

    /// This must be the first message from a session helper connection
    HelloSessionHelper { session_id: SessionId },

    /// Update from the session helper informing about the currently mounted ingest shares
    SessionUpdate { shares: Vec<SessionShare> },
}

// TODO: figure out relation to the ShareState type - does that go away with the new design?
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shares {
    /// The foreground share ID is needed to have the notification tray UI present it
    foreground_share_id: Option<String>,

    /// All shares that should be mounted
    shares: Vec<Share>,
}

/// Updates sent to the session helper and the GUI
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InternalMsgToSession {
    DesiredState {
        /// Configuration contains the samba hostname whose mounts are managed by the session helper
        /// (also the port if we are on Linux)
        config: SessionHelperConfig,

        /// None means we haven't reveiced the desired state yet. We need to consider this, because
        /// otherwise, it could lead to a flip-flopping situation when re-connecting. If this is
        /// None, and shares are mounted, no reconciliation should be happening.
        share_state: Option<Shares>,
    },
    /// Updated state which sessions are mounted in the given desktop session
    MountUpdate {
        session_id: SessionId,
        shares: Vec<MountedShare>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum EventAPIError {
    #[error("connection dropped")]
    Dropped,

    #[error("invalid message, dropping connection")]
    InvalidMsg,

    #[error("internal server error")]
    AxumError(#[from] axum::Error),

    #[error("serialization/message format error")]
    Serialization(#[from] serde_json::Error),

    #[error("internal communication error")]
    ChannelRecv(#[from] tokio::sync::broadcast::error::RecvError),

    #[error("internal communication error")]
    ChannelSend(#[from] tokio::sync::broadcast::error::SendError<StateUpdateToSession>),

    #[error("state error, message sent out of turn?")]
    StateError,
}

async fn recv_obj<T>(socket: &mut WebSocket) -> Result<T, EventAPIError>
where
    T: DeserializeOwned,
{
    loop {
        let msg = socket
            .recv()
            .await
            .transpose()?
            .ok_or(EventAPIError::Dropped)?;
        let msg_text = match &msg {
            Message::Text(utf8_bytes) => utf8_bytes.as_str(),
            Message::Ping(_a) | Message::Pong(_a) => continue,
            Message::Close(_close_frame) => return Err(EventAPIError::Dropped),
            _ => return Err(EventAPIError::InvalidMsg),
        };
        let msg_obj: T = serde_json::from_str(msg_text)?;

        return Ok(msg_obj);
    }
}

async fn send_obj<T>(socket: &mut WebSocket, obj: &T) -> Result<(), EventAPIError>
where
    T: Serialize,
{
    let msg = Message::Text(serde_json::to_string(obj)?.into());
    socket.send(msg).await?;

    Ok(())
}

async fn send_update_to_session_helper(
    update: StateUpdateToSession,
    config: &SessionHelperConfig,
    socket: &mut WebSocket,
) -> Result<(), EventAPIError> {
    match update {
        StateUpdateToSession::DesiredState {
            foreground_share_id,
            shares,
        } => {
            send_obj(
                socket,
                &InternalMsgToSession::DesiredState {
                    config: config.clone(),
                    share_state: Some(Shares {
                        foreground_share_id,
                        shares,
                    }),
                },
            )
            .await?;
        }
        StateUpdateToSession::MountUpdate {
            session_id: _,
            shares: _,
        } => {} // ignore
    }

    Ok(())
}

async fn record_update_from_session_helper(
    session_update: InternalMsgToAgent,
    tx: broadcast::Sender<StateUpdateToSession>,
    session_id: &SessionId,
    state: &mut AgentState,
) -> Result<(), EventAPIError> {
    match session_update {
        InternalMsgToAgent::HelloGui { session_id: _ }
        | InternalMsgToAgent::HelloSessionHelper { session_id: _ } => {
            Err(EventAPIError::StateError)
        }
        InternalMsgToAgent::SessionUpdate { shares } => {
            let a = state.mounted_shares.entry(session_id.clone()).or_default();
            let mounted_shares: Vec<MountedShare> = shares
                .into_iter()
                .map(|s| MountedShare {
                    share: s.share,
                    mount_path: s.path,
                })
                .collect();
            *a = mounted_shares.clone();
            // forward to the GUI
            tx.send(StateUpdateToSession::MountUpdate {
                session_id: session_id.clone(),
                shares: mounted_shares,
            })?;
            Ok(())
        }
    }
}

async fn handle_session_helper_websocket(
    state: Arc<Mutex<AgentState>>,
    mut socket: WebSocket,
    session_id: SessionId,
) -> Result<(), EventAPIError> {
    let _guard = SessionConnection::new(
        &session_id,
        state.lock().await.session_channel_sender.clone(),
    )
    .await;
    let (mut rx, tx, remote_info, smb_hostname, smb_port) = {
        let agent_state = state.lock().await;
        let receiver = agent_state.desired_state_channel.subscribe();

        (
            receiver,
            agent_state.desired_state_channel.clone(),
            agent_state.remote_info.clone(),
            agent_state.config.smb_hostname.clone(),
            agent_state.config.smb_port,
        )
    };

    let session_config = SessionHelperConfig {
        smb_hostname,
        smb_port,
    };

    // initial state
    send_obj(
        &mut socket,
        &InternalMsgToSession::DesiredState {
            config: session_config.clone(),
            share_state: remote_info.map(|remote| Shares {
                foreground_share_id: remote
                    .desired_shares
                    .foreground_share
                    .clone()
                    .map(|s| s.share_id),
                shares: remote.desired_shares.shares.clone(),
            }),
        },
    )
    .await?;

    loop {
        select! {
            state_update = rx.recv() => {
                let update = state_update?;
                send_update_to_session_helper(update, &session_config, &mut socket).await?;
            }
            session_update = recv_obj::<InternalMsgToAgent>(&mut socket) => {
                record_update_from_session_helper(session_update?, tx.clone(), &session_id, state.lock().await.deref_mut()).await?;
            }
        }
    }
    // TODO: ensure cleanup of session-keyed data on session helper termination (probably need to
    // wrap around this function and perform cleanup on either normal termination or any error)
}

async fn send_update_to_gui(
    update: StateUpdateToSession,
    config: &SessionHelperConfig,
    socket: &mut WebSocket,
) -> Result<(), EventAPIError> {
    let msg = match update {
        StateUpdateToSession::DesiredState {
            foreground_share_id,
            shares,
        } => InternalMsgToSession::DesiredState {
            config: config.clone(),
            share_state: Some(Shares {
                foreground_share_id,
                shares,
            }),
        },
        StateUpdateToSession::MountUpdate { session_id, shares } => {
            InternalMsgToSession::MountUpdate { session_id, shares }
        }
    };
    send_obj(socket, &msg).await?;
    Ok(())
}

async fn handle_gui_websocket(
    state: Arc<Mutex<AgentState>>,
    mut socket: WebSocket,
    session_id: SessionId,
) -> Result<(), EventAPIError> {
    let (mut rx, _tx, remote_info, smb_hostname, smb_port, mounted_shares) = {
        let agent_state = state.lock().await;
        let receiver = agent_state.desired_state_channel.subscribe();
        let mounted_shares = agent_state.mounted_shares.get(&session_id).cloned();

        (
            receiver,
            agent_state.desired_state_channel.clone(),
            agent_state.remote_info.clone(),
            agent_state.config.smb_hostname.clone(),
            agent_state.config.smb_port,
            mounted_shares,
        )
    };

    let session_config = SessionHelperConfig {
        smb_hostname,
        smb_port,
    };

    // initial state
    send_obj(
        &mut socket,
        &InternalMsgToSession::DesiredState {
            config: session_config.clone(),
            share_state: remote_info.map(|remote| Shares {
                foreground_share_id: remote
                    .desired_shares
                    .foreground_share
                    .clone()
                    .map(|s| s.share_id),
                shares: remote.desired_shares.shares.clone(),
            }),
        },
    )
    .await?;
    if let Some(mounted) = mounted_shares {
        send_obj(
            &mut socket,
            &InternalMsgToSession::MountUpdate {
                session_id,
                shares: mounted,
            },
        )
        .await?;
    }

    loop {
        select! {
            state_update = rx.recv() => {
                let update = state_update?;
                send_update_to_gui(update, &session_config, &mut socket).await?;
            }
            session_update = recv_obj::<InternalMsgToAgent>(&mut socket) => {
                // panic! or rather, don't. the GUI isn't supposed to send us anything,
                // but if it does, just log and continue.
                // TODO: is this needed to drain the websocket?
                // TODO: should we just drop "GUI" connections that do unexpected stuff like this?
                warn!("GUI sent us something unexpected over the websocket connection: {session_update:?}");
            }
        }
    }
}

#[derive(Debug)]
pub enum SessionBeginEnd {
    Begin { session_id: SessionId },
    End { session_id: SessionId },
}

/// Session connection guard; when dropped, we send a message via the MPSC
/// channel which triggers cleanup in `agent_state::session_bookkeeper`
#[derive(Debug)]
struct SessionConnection {
    session_id: SessionId,
    channel: mpsc::UnboundedSender<SessionBeginEnd>,
}

impl SessionConnection {
    #[must_use]
    pub async fn new(
        session_id: &SessionId,
        channel: mpsc::UnboundedSender<SessionBeginEnd>,
    ) -> Self {
        #[allow(clippy::expect_used)]
        channel
            .send(SessionBeginEnd::Begin {
                session_id: session_id.clone(),
            })
            .expect("the session bookkeeping channel should be alive");
        SessionConnection {
            session_id: session_id.clone(),
            channel,
        }
    }
}

impl Drop for SessionConnection {
    fn drop(&mut self) {
        #[allow(clippy::expect_used)]
        self.channel
            .send(SessionBeginEnd::End {
                session_id: self.session_id.clone(),
            })
            .expect("session cleanup channel should be alive");
    }
}

async fn handle_connection(
    state: Arc<Mutex<AgentState>>,
    mut socket: WebSocket,
) -> Result<(), EventAPIError> {
    let msg_obj: InternalMsgToAgent = recv_obj(&mut socket).await?;

    match msg_obj {
        InternalMsgToAgent::HelloGui { session_id } => {
            handle_gui_websocket(state, socket, session_id).await?;
        }
        InternalMsgToAgent::HelloSessionHelper { session_id } => {
            handle_session_helper_websocket(state, socket, session_id).await?;
        }
        InternalMsgToAgent::SessionUpdate { shares: _ } => {
            return Err(EventAPIError::InvalidMsg);
        }
    }

    Ok(())
}

pub async fn events_api(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
) -> Response {
    // Disallow browsers from connecting. Our own clients don't send an Origin header.
    // this assumes that if the browser is too old to send an Origin header (IE10 or older),
    // it will not be connected to the internet, or at least not to untrusted websites.
    if headers.get("origin").is_some() {
        return StatusCode::FORBIDDEN.into_response();
    }

    ws.on_upgrade(async move |socket| {
        if let Err(e) = handle_connection(Arc::clone(&state.state), socket).await {
            error!("dropping websocket connection: {e}");
        }
    })
    .into_response()
}
