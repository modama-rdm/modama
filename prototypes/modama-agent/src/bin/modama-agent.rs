use std::{fs::File, io::BufReader, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use futures_util::{future::join_all, StreamExt};
use log::{error, info, warn};
use modama_agent::{
    common::init_logging,
    config::{AgentConfig, AgentLinuxConfig},
    messages::{CurrentForegroundShare, EventChannelMessage},
    platform::{cleanup_mount_points, ensure_mounted, umount_old_shares},
    share::{MountedShare, Share},
};
use rustls_pemfile::{certs, private_key};
use tokio::sync::{Mutex, OnceCell};
use tokio_rustls::rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    ClientConfig, RootCertStore,
};
use tokio_tungstenite::{connect_async, connect_async_tls_with_config};
use url::Url;

#[derive(Debug, Default)]
enum ConnectionState {
    #[default]
    Connecting,
    Connected,
}

#[derive(Debug, Clone)]
struct ShareState<T>
where
    T: PartialEq + Clone,
{
    /// The current foreground share, if any.
    foreground_share: Option<T>,

    /// All shares, including the foreground share
    shares: Vec<T>,
}

impl<T> ShareState<T>
where
    T: PartialEq + Clone,
{
    fn set_foreground_share(&mut self, share: Option<T>) {
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

    fn set_shares(&mut self, shares: Vec<T>) {
        self.shares = shares;
        self.ensure_consistent();
    }
}

impl<T> Default for ShareState<T>
where
    T: PartialEq + Clone,
{
    fn default() -> Self {
        Self {
            foreground_share: None,
            shares: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct AgentState {
    connection: ConnectionState,

    current_shares: ShareState<MountedShare>,
    desired_shares: ShareState<Share>,

    /// Runtime configuration
    config: AgentConfig,
}

impl AgentState {
    fn new(config: &AgentConfig) -> Self {
        let config = config.clone();
        Self {
            connection: Default::default(),
            current_shares: Default::default(),
            desired_shares: Default::default(),
            config,
        }
    }
}

async fn update_share_list(
    state: &mut AgentState,
    desired_shares: ShareState<Share>,
) -> Result<()> {
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

    umount_old_shares(&state.config, &state.current_shares.shares).await;
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
            let desired_shares: ShareState<Share> = ShareState {
                foreground_share: foreground_share.map(Into::into),
                shares: shares.into_iter().map(Into::into).collect(),
            };
            state.lock().await.desired_shares = desired_shares.clone();
            update_share_list(&mut *state.lock().await, desired_shares).await?;
            Ok(())
        }
    }
}

fn get_root_cert_store() -> Result<RootCertStore> {
    let mut root_cert_store = RootCertStore::empty();

    let ca_certs = File::open("./certs/ca/ca.cert.pem")?;
    for cert in certs(&mut BufReader::new(ca_certs)) {
        root_cert_store.add(cert?)?;
    }

    Ok(root_cert_store)
}

fn get_client_chain() -> Result<Vec<CertificateDer<'static>>> {
    let cert_file = File::open("./certs/clients/holo/dld.chain.pem")?;

    // FIXME: fail instead of flattening
    Ok(certs(&mut BufReader::new(cert_file)).flatten().collect())
}

fn get_client_key() -> Result<PrivateKeyDer<'static>> {
    let key_file = File::open("./certs/clients/holo/dld.priv.pem")?;
    let pk = private_key(&mut BufReader::new(key_file))?.expect("private key file contains a key");
    Ok(pk)
}

async fn message_loop(state: Arc<Mutex<AgentState>>) {
    let config = state.lock().await.config.clone();

    let root_store = get_root_cert_store().unwrap();
    let tls_config = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(get_client_chain().unwrap(), get_client_key().unwrap())
            .unwrap(),
    );

    loop {
        state.lock().await.connection = ConnectionState::Connecting;
        let (conn, _response) = match connect_async_tls_with_config(
            &config.api_url,
            None,
            false,
            Some(tokio_tungstenite::Connector::Rustls(Arc::clone(&tls_config))),
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
    Json(
        match &api_state.state.lock().await.current_shares.foreground_share {
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
        },
    )
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
        api_url: Url::parse("wss://localhost:1234").expect("invalid websocket URL"),
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
