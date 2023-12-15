use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use futures_util::{future::join_all, StreamExt};
use log::{debug, error, info, warn};
use modama_agent::messages::{CurrentForegroundShare, EventChannelMessage, ShareMsg};
use nix::mount::MsFlags;
use procfs::process::Process;
use thiserror::Error;
use tokio::sync::{Mutex, OnceCell};
use tokio_tungstenite::connect_async;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Share {
    /// A UUID for this share
    share_id: String,

    /// The full share name; the first path element of the SMB URL
    share_name: String,

    /// The path in the data management that is exported with this Share
    dest_path: String,

    /// When was this share created?
    created_timestamp: SystemTime,

    /// Username for connecting (temporary)
    username: String,

    /// Password for connecting (temporary)
    password: String,
}

impl From<ShareMsg> for Share {
    fn from(value: ShareMsg) -> Self {
        Self {
            share_id: value.share_id,
            share_name: value.share_name,
            dest_path: value.dest_path,
            created_timestamp: UNIX_EPOCH + Duration::from_secs(value.created_timestamp),
            username: value.username,
            password: value.password,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MountedShare {
    share: Share,

    /// The mount path; a drive letter on Windows, or a path on Linux
    mount_path: PathBuf,
}

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
            mount_point_prefix: PathBuf::from("/mnt/"),
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
            "Could not canonicalize either {} or {}; falling back to strict equality: {e}",
            m1.display(),
            m2.display()
        );
        m1 == m2
    })
}

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
    {}

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

async fn ensure_umounted(share: &MountedShare, _config: &AgentConfig) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        Ok(nix::mount::umount(&share.mount_path)?)
    }
}

async fn handle_message(state: &Arc<Mutex<AgentState>>, msg: &str) {
    let event_msg: EventChannelMessage = match serde_json::from_str(msg) {
        Ok(m) => m,
        Err(e) => {
            error!("Invalid message encountered, ignoring: {e}");
            return;
        }
    };

    match event_msg {
        EventChannelMessage::UpdateShareList {
            shares,
            foreground_share,
        } => {
            let mut state_guard = state.lock().await;
            let new_fg_share: Option<Share> = foreground_share.map(Into::into);
            let old_fg_share: Option<&Share> =
                state_guard.foreground_share.as_ref().map(|ms| &ms.share);
            if old_fg_share != new_fg_share.as_ref() {
                if let Some(share) = new_fg_share {
                    state_guard.foreground_share =
                        match ensure_mounted(&share, &state_guard.config).await {
                            Ok(s) => Some(s),
                            Err(e) => {
                                error!("Mount failed: {e}");
                                None
                            }
                        }
                } else {
                    state_guard.foreground_share = None;
                }
            }
            state_guard.shares = join_all(
                shares
                    .into_iter()
                    .map(|s| async { ensure_mounted(&s.into(), &state_guard.config).await }),
            )
            .await
            .into_iter()
            .flatten() // get rid of the `None`s
            .collect();
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
                        handle_message(&state, &msg).await;
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
    let env = env_logger::Env::default()
        .filter_or("MODAMA_AGENT_LOG_LEVEL", "info")
        .write_style("always");
    env_logger::init_from_env(env);

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

    // username: sharef76b764d16a043ea805918305ce
    // password: kubeFWCBMvw1AVTADZsM4wEXxECwBxgAAo8_9cAZ_5U
    // share: share-c3b89492-a384-45c3-bbb0-1405cd3c63d1
    // hostname: moellenstedt.iff.kfa-juelich.de
    let test_share = Share {
        share_id: "c3b89492-a384-45c3-bbb0-1405cd3c63d1".to_owned(),
        share_name: "share-c3b89492-a384-45c3-bbb0-1405cd3c63d1".to_owned(),
        dest_path: "/adhoc/datamanagement/2023-11-06-ingest-stuff/".to_owned(),
        created_timestamp: SystemTime::now(),
        username: "sharef76b764d16a043ea805918305ce".to_owned(),
        password: "kubeFWCBMvw1AVTADZsM4wEXxECwBxgAAo8_9cAZ_5U".to_owned(),
    };
    println!("{:?}", is_mounted(&test_share, config).unwrap());
    let mounted_test_share = ensure_mounted(&test_share, config).await.unwrap();
    println!("{:?}", mounted_test_share);
    println!("{:?}", is_mounted(&test_share, config).unwrap());
    println!("{:?}", ensure_umounted(&mounted_test_share, config).await);
    println!("{:?}", is_mounted(&test_share, config).unwrap());

    tokio::spawn(serve_local_web_api(Arc::clone(&state)));
    message_loop(Arc::clone(&state)).await;
}
