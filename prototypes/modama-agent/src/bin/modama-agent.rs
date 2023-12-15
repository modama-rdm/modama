use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use futures_util::{future::join_all, StreamExt};
use log::{debug, error, info, warn};
use modama_agent::{
    common::init_logging,
    messages::{CurrentForegroundShare, EventChannelMessage, ShareMsg},
    share::{MountedShare, Share},
};
use nix::mount::MsFlags;
use procfs::process::Process;
use thiserror::Error;
use tokio::sync::{Mutex, OnceCell};
use tokio_tungstenite::connect_async;
use url::Url;

#[derive(Debug, Default)]
enum ConnectionState {
    #[default]
    Connecting,
    Connected,
}

#[derive(Debug, Clone)]
struct AgentLinuxConfig {
    pub mount_point_prefix: PathBuf,

    // we assume that this system is using some shared unix user,
    // which should have access to the ingest share(s):
    pub uid: u32,
    pub gid: u32,
}

impl Default for AgentLinuxConfig {
    fn default() -> Self {
        Self {
            mount_point_prefix: PathBuf::from("/run/modama/ingest/"),
            uid: 1000,
            gid: 1000,
        }
    }
}

#[derive(Debug, Clone)]
struct AgentConfig {
    /// The base URL for the websocket API (ws:// or wss://)
    pub api_url: Url,

    /// The hostname of the ingest Samba server
    pub smb_hostname: String,

    /// The resolved IP address of the Samba server (lazily looked up on first mount)
    pub smb_ip: OnceCell<String>,

    /// The Samba domain to use for mounting
    pub smb_domain: String,

    /// A user-readable short description of the data producer, like "DLD"
    pub agent_name: String,

    /// A short name of the resouce this agent belongs to, for example "Holo"
    pub resource_name: String,

    // FIXME: find a better way?
    pub linux_config: Option<AgentLinuxConfig>,
    // TODO: add mTLS certs/keys etc. for resource identification and auth
}

#[derive(Error, Debug)]
pub enum DNSLookupFailure {
    #[error("No address found for `{hostname}`")]
    NoAddressFound { hostname: String },
}

impl AgentConfig {
    pub async fn get_smb_ip(&self) -> Result<&str> {
        self.smb_ip
            .get_or_try_init(|| async {
                let mut addrs = async_dns::lookup(&self.smb_hostname).await?;
                let first = addrs.next().ok_or(DNSLookupFailure::NoAddressFound {
                    hostname: self.smb_hostname.to_owned(),
                })?;
                Ok(first.ip_address.to_string())
            })
            .await
            .map(String::as_str)
    }

    pub fn get_linux_config(&self) -> AgentLinuxConfig {
        let c = &self.linux_config;
        c.clone().unwrap_or_default()
    }
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
fn mount_points_equal(m1: &Path, m2: &Path) -> bool {
    (|| {
        let p1 = m1.canonicalize()?;
        let p2 = m2.canonicalize()?;
        Ok::<_, std::io::Error>(p1 == p2)
    })()
    .unwrap_or_else(|e| {
        info!(
            "Could not canonicalize either `{}` or `{}`; falling back to strict equality: {e}",
            m1.display(),
            m2.display()
        );
        m1 == m2
    })
}

#[cfg(target_os = "linux")]
fn is_mounted(share: &Share, config: &AgentConfig) -> Result<bool> {
    let mounts = Process::myself()?.mountinfo()?;
    let target = target_for_share(share, config);

    Ok(mounts.into_iter().any(|m| {
        debug!(
            "source: {:?} mount point: {:?}",
            m.mount_source, m.mount_point
        );
        mount_points_equal(&m.mount_point, &target)
    }))
}

#[cfg(target_os = "linux")]
fn target_for_share(share: &Share, config: &AgentConfig) -> PathBuf {
    config
        .get_linux_config()
        .mount_point_prefix
        .join(&share.share_name)
}

/// Ensure that `share` is mounted, and return a `MountedShare` if the mount succeeded.
async fn ensure_mounted(share: &Share, config: &AgentConfig) -> Result<MountedShare> {
    #[cfg(target_os = "windows")]
    {
        todo!();
    }

    #[cfg(target_os = "linux")]
    {
        // On Linux, we have to:
        //
        // - take care of creating mount points
        // - check if the mount already exists (here we assume that an existing
        //   mount with the given share name )
        // - specific config for UID/GID handling
        // - perform IP address lookup, as this is done in userspace, not by the kernel

        let linux_config = match &config.linux_config {
            None => AgentLinuxConfig::default(),
            Some(c) => c.clone(),
        };

        let source = format!("//{}/{}", &config.smb_hostname, share.share_name);
        let target = target_for_share(share, config);

        if is_mounted(share, config)? {
            return Ok(MountedShare {
                share: share.clone(),
                mount_path: target,
            });
        }

        let options = format!(
            "domain={},username={},password={},uid={},gid={},forceuid,forcegid,vers=3.1.1,ip={}",
            &config.smb_domain,
            &share.username,
            &share.password,
            &linux_config.uid,
            linux_config.gid,
            &config.get_smb_ip().await?,
        );

        if !target.exists() {
            std::fs::create_dir_all(&target).unwrap();
        }

        info!("Mounting {} at {}", &source, &target.to_string_lossy());

        nix::mount::mount::<str, Path, str, str>(
            Some(source.as_ref()),
            &target,
            Some("cifs"),
            MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RELATIME,
            Some(options.as_ref()),
        )
        .map(|_| MountedShare {
            share: share.clone(),
            mount_path: target,
        })
        .with_context(|| format!("Could not mount {}", &source))
    }
}

#[cfg(target_os = "linux")]
async fn ensure_umounted(mount_point: &Path, _config: &AgentConfig) -> Result<()> {
    {
        info!("Umounting {}", &mount_point.to_string_lossy());
        nix::mount::umount(mount_point)
            .with_context(|| format!("Umounting {}", &mount_point.to_string_lossy()))?;
        std::fs::remove_dir(mount_point)
            .with_context(|| format!("Unlinking mountpoint {}", &mount_point.to_string_lossy()))?;
        Ok(())
    }
}

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

    // now collect any shares that are mounted under the prefix which are _not_
    // in the list of shares:
    let mounts = Process::myself()?.mountinfo()?;
    let should_be_mounted: HashSet<PathBuf> =
        state.shares.iter().map(|s| s.mount_path.clone()).collect();
    let config = &state.config;
    let mounts_in_prefix: HashSet<_> = mounts
        .into_iter()
        .flat_map(|m| m.mount_point.canonicalize().ok())
        .filter(|m| m.starts_with(state.config.get_linux_config().mount_point_prefix))
        .collect();
    join_all(
        mounts_in_prefix
            .clone()
            .into_iter()
            .filter(|m| !should_be_mounted.contains(m))
            .map(|m| async move { ensure_umounted(&m, config).await }),
    )
    .await
    .into_iter()
    .for_each(|result| {
        if let Err(err) = result {
            warn!("Umounting failed: {err}");
        }
    });

    // clean up ingest mount prefix directory,
    // remove all directories that are not currently active mount points:
    std::fs::read_dir(state.config.get_linux_config().mount_point_prefix)?
        .flatten()
        .filter(|dir_entry| {
            if let Ok(metadata) = std::fs::metadata(dir_entry.path()) {
                metadata.is_dir()
            } else {
                false
            }
        })
        .map(|dir_entry| dir_entry.path())
        .flat_map(|path| path.canonicalize().ok())
        .filter(|path| ! mounts_in_prefix.contains(path))
        .for_each(|path| if let Err(err) = std::fs::remove_dir(&path) {
            warn!("Cannot clean up {}: {err}", &path.to_string_lossy());
        } else {
            info!("Cleaned up unused mount point {}", &path.to_string_lossy());
        });

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
