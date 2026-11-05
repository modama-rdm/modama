use itertools::Itertools;
use log::{debug, error, info, warn};
use modama_common::messages::Share;
use rand::{RngExt, distr::Alphanumeric};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    io,
    marker::Send,
    option::Option,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    fs::{File, rename},
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{
        Mutex, OnceCell,
        mpsc::{Sender, error::SendError},
    },
    task::{JoinError, spawn_blocking},
};

use crate::{
    mount::{BindMount, MountBackend, MountError},
    mount_manager::{MountManager, SetStateError},
    process_manager::{ProcessManager, ProcessManagerCommand},
    samba_config::{SambaConfigManager, UpdateSharesError},
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct User {
    pub uid: u32,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("User")
            .field("uid", &self.uid)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum CreateUserError {
    #[error("failed to add {username}: {stderr}")]
    Usermgmt { username: String, stderr: String },

    #[error("failed to call smbpasswd")]
    SmbpasswdError,

    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),
}

pub trait UserProvider: Clone + Sync + Send + 'static {
    fn passwd_from_name(&self, name: impl AsRef<CStr>) -> io::Result<Option<etc_passwd::Passwd>>;
    fn passwd_from_uid(&self, uid: u32) -> io::Result<Option<etc_passwd::Passwd>>;
    fn add_system_user(
        &self,
        user: &User,
    ) -> impl std::future::Future<Output = Result<(), CreateUserError>> + Send;
    fn add_samba_user(
        &self,
        user: &User,
    ) -> impl std::future::Future<Output = Result<(), CreateUserError>> + Send;

    fn remove_system_user(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<(), CreateUserError>> + Send;
    fn remove_samba_user(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<(), CreateUserError>> + Send;

    fn passwd_from_name_string(&self, name: &str) -> io::Result<Option<etc_passwd::Passwd>> {
        let c_username = CString::new(name.as_bytes()).map_err(|_e| {
            io::Error::new(io::ErrorKind::InvalidData, "nul encountered in username")
        })?;
        self.passwd_from_name(&c_username)
    }
}

#[derive(Clone)]
pub struct DefaultUserProvider {}

impl DefaultUserProvider {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for DefaultUserProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl UserProvider for DefaultUserProvider {
    fn passwd_from_name(&self, name: impl AsRef<CStr>) -> io::Result<Option<etc_passwd::Passwd>> {
        etc_passwd::Passwd::from_name(name)
    }

    fn passwd_from_uid(&self, uid: u32) -> io::Result<Option<etc_passwd::Passwd>> {
        etc_passwd::Passwd::from_uid(uid)
    }

    async fn add_system_user(&self, user: &User) -> Result<(), CreateUserError> {
        let useradd_res = Command::new("useradd")
            .args([
                "--no-create-home",
                "--no-user-group",
                &user.username,
                "--badname", // ignore system settings, for example length limit
                "--uid",
                &user.uid.to_string(),
                "--home-dir",
                "/tmp",
            ])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await?;
        if !useradd_res.status.success() {
            let stderr = String::from_utf8_lossy(&useradd_res.stderr);
            warn!("useradd failed; stderr:\n {}", stderr);
            let stdout = String::from_utf8_lossy(&useradd_res.stdout);
            debug!("stdout from useradd:\n {}", stdout);
            return Err(CreateUserError::Usermgmt {
                username: user.username.clone(),
                stderr: stderr.to_string(),
            });
        }

        Ok(())
    }

    async fn add_samba_user(&self, user: &User) -> Result<(), CreateUserError> {
        let mut smbpasswd = Command::new("smbpasswd")
            .args(["-s", "-a", &user.username])
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdin = smbpasswd
            .stdin
            .take()
            .ok_or(CreateUserError::SmbpasswdError)?;
        let smbpasswd_in: String = format!("{}\n{}\n", user.password, user.password);
        stdin.write_all(smbpasswd_in.as_bytes()).await?;
        let res = smbpasswd.wait_with_output().await?;
        if !res.status.success() {
            let stderr = String::from_utf8_lossy(&res.stderr);
            warn!("smbpasswd failed; stderr:\n {}", stderr);
            let stdout = String::from_utf8_lossy(&res.stdout);
            debug!("stdout from smbpasswd:\n {}", stdout);
            return Err(CreateUserError::Usermgmt {
                username: user.username.clone(),
                stderr: stderr.to_string(),
            });
        }

        Ok(())
    }

    async fn remove_system_user(&self, username: &str) -> Result<(), CreateUserError> {
        let userdel_res = Command::new("userdel")
            .args([
                "-f", // force, even if there's a process from the user still running
                username,
            ])
            .stdin(Stdio::null())
            .output()
            .await?;
        if !userdel_res.status.success() {
            let stderr = String::from_utf8_lossy(&userdel_res.stderr);
            warn!("userdel failed; stderr:\n {}", stderr);
            let stdout = String::from_utf8_lossy(&userdel_res.stdout);
            debug!("stdout from userdel:\n {}", stdout);
            return Err(CreateUserError::Usermgmt {
                username: username.to_string(),
                stderr: stderr.to_string(),
            });
        }

        Ok(())
    }

    async fn remove_samba_user(&self, username: &str) -> Result<(), CreateUserError> {
        let smbpasswd_res = Command::new("smbpasswd")
            .args(["-x", username])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await?;
        if !smbpasswd_res.status.success() {
            let stderr = String::from_utf8_lossy(&smbpasswd_res.stderr);
            warn!("smbpasswd failed; stderr:\n {}", stderr);
            let stdout = String::from_utf8_lossy(&smbpasswd_res.stdout);
            debug!("stdout from smbpasswd:\n {}", stdout);
            return Err(CreateUserError::Usermgmt {
                username: username.to_string(),
                stderr: stderr.to_string(),
            });
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SmbDropState {
    pub shares: HashMap<String, Share>,
    pub users: HashMap<String, User>,
}

impl SmbDropState {
    pub fn empty() -> Self {
        Self {
            shares: Default::default(),
            users: Default::default(),
        }
    }

    /// Do a consistency check for the config and the user provider state.
    ///
    /// Right now, this should not be used as a hard failure, as we might not
    /// always be able to guarantee consistency. On a higher level,
    /// `ShareManager::try_heal` should eventually make sure the state converges on
    /// something consistent.
    pub fn check_consistent(&self, user_provider: &impl UserProvider) -> Result<(), ShareError> {
        let share_uids: HashSet<u32> = self.shares.values().map(|s| s.uid).collect();
        let user_uids: HashSet<u32> = self.users.values().map(|u| u.uid).collect();
        if share_uids != user_uids {
            let share_uids: Vec<u32> = share_uids.iter().sorted().cloned().collect();
            let user_uids: Vec<u32> = user_uids.iter().sorted().cloned().collect();
            return Err(ShareError::Inconsistent(format!(
                "mismatch in UIDs of shares ({share_uids:?}) and users ({user_uids:?})"
            )));
        }

        for (_uid, user) in self.users.iter().sorted() {
            let pwd = user_provider
                .passwd_from_name_string(&user.username)
                .map_err(|e| ShareError::IoError {
                    source: e,
                    msg: format!("look up username {}", user.username),
                })?;
            if let Some(pwd) = pwd {
                if pwd.uid != user.uid {
                    return Err(ShareError::Inconsistent(format!(
                        "uid mismatch: {} (system) vs {} (share config)",
                        pwd.uid, user.uid
                    )));
                }
            } else {
                return Err(ShareError::Inconsistent(format!(
                    "user `{}` not found",
                    user.username
                )));
            }

            let pwd = user_provider
                .passwd_from_uid(user.uid)
                .map_err(|e| ShareError::IoError {
                    source: e,
                    msg: format!("look up uid {}", user.uid),
                })?;
            if let Some(pwd) = pwd {
                let pwd_name = pwd.name.to_string_lossy();
                if pwd_name != user.username {
                    return Err(ShareError::Inconsistent(format!(
                        "username mismatch for {}: `{}` (system) vs `{}` (share config)",
                        user.uid, pwd_name, user.username
                    )));
                }
            } else {
                return Err(ShareError::Inconsistent(format!(
                    "user for id {} not found",
                    user.uid
                )));
            }

            for share in self.shares.values() {
                let user_entry = if let Some(user) = self.users.get(&share.uid.to_string()) {
                    user
                } else {
                    return Err(ShareError::Inconsistent(format!(
                        "user {} for share `{}` not found in config",
                        share.uid, share.share_id
                    )));
                };
                if user_entry.username != share.username {
                    return Err(ShareError::Inconsistent(format!(
                        "username mismatch for {}: user entry `{}` vs share `{}`",
                        user_entry.uid, user_entry.username, share.username
                    )));
                }
            }
        }

        // all shares attached to a resource belong to one uid (see `detach_other_uids`);
        // possible healing: keep the newest share's uid, detach the others. Not done, because
        // the next share created for the resource fixes it anyway.
        let mut owners: HashMap<&str, u32> = HashMap::new();
        for share in self.shares.values().sorted_by_key(|s| &s.share_id) {
            if let Some(resource) = &share.resource
                && let Some(owner) = owners.insert(resource, share.uid)
                && owner != share.uid
            {
                return Err(ShareError::Inconsistent(format!(
                    "resource `{resource}` has shares of uids {owner} and {}",
                    share.uid
                )));
            }
        }

        Ok(())
    }

    /// Get the User for the given uid, or create a new one with a random password.
    /// Note that this will not create a system or samba user;
    /// this is done by the `ShareManager::create_share`.
    pub async fn get_or_create_user(
        &mut self,
        uid: u32,
        share_id: String,
    ) -> Result<User, ShareError> {
        self.get_or_create_user_with_password(uid, share_id, None)
            .await
    }

    /// Same as `get_or_create_user`, but pre-define the password that is
    /// used when creating (it's not used for updating the password in case
    /// the user already exists!)
    pub async fn get_or_create_user_with_password(
        &mut self,
        uid: u32,
        share_id: String,
        password: Option<&str>,
    ) -> Result<User, ShareError> {
        let key = uid.to_string();
        if let Some(user) = self.users.get(&key) {
            debug!("get_or_create_user: returning existing user for {key}");
            Ok((*user).clone())
        } else {
            if share_id.len() < 36 {
                return Err(ShareError::InvalidShareId(share_id));
            }
            let mut username = format!("share{share_id}").replace('-', "");
            let _rest = username.split_off(31);
            debug!("get_or_create_user: adding user {username}");
            let password = if let Some(password) = password {
                password.to_owned()
            } else {
                make_password()
            };
            let user = User {
                uid,
                username,
                password,
            };
            self.users.insert(key, user.clone());
            Ok(user)
        }
    }

    pub async fn add_share(&mut self, share: Share) -> Result<(), ShareError> {
        let key = share.share_id.clone();
        self.shares.insert(key, share);
        Ok(())
    }

    /// Detach the shares of other uids from `resource`: all shares attached to a resource
    /// belong to one uid, so an agent never gets shares with different credentials (Windows
    /// allows only one set per server). Detached shares stay; they are just no longer sent to
    /// the resource's agents.
    pub fn detach_other_uids(&mut self, resource: &str, uid: u32) {
        for share in self.shares.values_mut() {
            if share.resource.as_deref() == Some(resource) && share.uid != uid {
                info!(
                    "detaching share {} of uid {} from resource {resource}",
                    share.share_id, share.uid
                );
                share.resource = None;
            }
        }
    }

    pub async fn remove_share(&mut self, share_id: &str) -> Result<Share, ShareError> {
        info!("removing share {share_id}");
        if let Some(share) = self.shares.remove(share_id) {
            Ok(share)
        } else {
            Err(ShareError::ShareNotFound(share_id.to_owned()))
        }
    }

    /// Remove shares that are broken in some way, for example those that
    /// point to symlinks or non-directories (normal files, special files etc.).
    /// The idea is that over time, invalid shares are removed from the system.
    pub fn clean_shares(&mut self) {
        self.shares.retain(|_share_id, share| {
            let dest_path = share.get_dest_path();
            dest_path.is_dir() && !dest_path.is_symlink()
        });
    }

    /// Remove users that match the given UID
    pub fn remove_user(&mut self, uid_to_remove: u32) {
        self.users
            .retain(|_user_id, user| uid_to_remove != user.uid);
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ShareError {
    #[error("I/O error: {source} ({msg})")]
    IoError { source: io::Error, msg: String },

    #[error("serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("background task failed: {0}")]
    JoinError(#[from] JoinError),

    #[error("invalid share id: {0}")]
    InvalidShareId(String),

    #[error("Path name can only contain characters /, a-z, A-Z, 0-9, '_', '-', ' ' and '.'.")]
    PathError,

    #[error("share `{0}` not found")]
    ShareNotFound(String),

    #[error("inconsistent configuration: {0}")]
    Inconsistent(String),

    #[error("could not sync mount state: {0}")]
    StateError(#[from] SetStateError),

    #[error("mount error: {0}")]
    MountError(#[from] MountError),

    // FIXME: get rid of this one; always keep detailed error around
    #[error("other mount error")]
    OtherMountError,

    #[error("could not update shares: {0}")]
    UpdateShares(#[from] UpdateSharesError),

    #[error("could not reload samba: {0}")]
    SambaError(#[from] SendError<ProcessManagerCommand>),

    #[error("could not set smb password")]
    SmbpasswdError,

    #[error("could not add or remove system user {username}: {stderr}")]
    UsermgmtError { username: String, stderr: String },

    #[error("process manager in bad shape: {0}")]
    ProcessManagerError(String),

    #[error("could not write temporary config: {0}")]
    TempConfig(#[from] TempConfigError),

    #[error("issues with system clock; before UNIX_EPOCH?")]
    ClockIssues,
}

impl From<CreateUserError> for ShareError {
    fn from(value: CreateUserError) -> Self {
        match value {
            CreateUserError::Usermgmt { username, stderr } => {
                Self::UsermgmtError { username, stderr }
            }
            CreateUserError::IO(error) => Self::IoError {
                source: error,
                msg: "create user".to_owned(),
            },
            CreateUserError::SmbpasswdError => Self::SmbpasswdError,
        }
    }
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum TempConfigError {
    #[error("path must have a parent")]
    MustHaveParent,

    #[error("path must point to a file")]
    MustBeFile,

    #[error("path is not valid unicode")]
    InvalidUnicode,
}

/// Return a path to a randomized temporary filename in the same directory as
/// the file pointed to by the given `path`.
pub fn make_temporary_fname(path: &Path) -> Result<PathBuf, TempConfigError> {
    let dirname = path.parent().ok_or(TempConfigError::MustBeFile)?;
    let filename = path
        .file_name()
        .ok_or(TempConfigError::MustBeFile)?
        .to_str()
        .ok_or(TempConfigError::InvalidUnicode)?;
    loop {
        let rnd: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(15)
            .map(char::from)
            .collect();
        let rand_name = dirname.join(format!("{}-tmp{}", filename, rnd));
        let path = dirname.join(rand_name);
        if !path.exists() {
            return Ok(path);
        }
    }
}

fn make_password() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                             abcdefghijklmnopqrstuvwxyz\
                             0123456789+/";
    const PASSWORD_LEN: usize = 32;
    let mut rng = rand::rng();

    (0..PASSWORD_LEN)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Utility to manipulate on-disk configuration for smbdrop. Read-modify-write
/// cycles should be guarded by an external mutex.
#[derive(Clone, Debug)]
pub struct ShareConfigFile<U: UserProvider> {
    path: PathBuf,
    user_provider: U,
}

impl<U: UserProvider> ShareConfigFile<U> {
    fn new(path: PathBuf, user_provider: U) -> Self {
        Self {
            path,
            user_provider,
        }
    }

    async fn read_config_or_default(&self) -> Result<SmbDropState, ShareError> {
        if !self.path.exists() {
            Ok(SmbDropState::empty())
        } else {
            self.read_config().await
        }
    }

    async fn read_config(&self) -> Result<SmbDropState, ShareError> {
        let mut f = File::open(&self.path)
            .await
            .map_err(|e| ShareError::IoError {
                source: e,
                msg: format!("open config {:?} for reading", self.path),
            })?;
        let mut contents = Vec::new();
        f.read_to_end(&mut contents)
            .await
            .map_err(|e| ShareError::IoError {
                source: e,
                msg: format!("read config {:?} read_to_end", self.path),
            })?;

        let config = spawn_blocking(move || -> Result<_, serde_json::Error> {
            serde_json::from_slice(&contents)
        })
        .await??;

        Ok(config)
    }

    /// Write config without consistency checks. Useful for testing.
    pub async fn write_config_no_check(&self, config: &SmbDropState) -> Result<(), ShareError> {
        debug!("write_config");
        let config = config.clone();
        debug!("new config: {:?}", config);

        let serialized =
            spawn_blocking(move || -> Result<_, serde_json::Error> { serde_json::to_vec(&config) })
                .await??;

        let rand_name = make_temporary_fname(&self.path)?;
        debug!(
            "serialized config: {}",
            String::from_utf8_lossy(&serialized)
        );
        debug!("write_config: writing to {rand_name:?}");
        let mut opts = File::options();
        opts.mode(0o600).create_new(true).write(true);
        let mut f = opts
            .open(&rand_name)
            .await
            .map_err(|e| ShareError::IoError {
                source: e,
                msg: format!("open config {:?} for writing/creating", rand_name),
            })?;

        f.write_all(&serialized)
            .await
            .map_err(|e| ShareError::IoError {
                source: e,
                msg: format!("write to {:?}", rand_name),
            })?;
        debug!("write_config: wrote all...");
        f.sync_all().await.map_err(|e| ShareError::IoError {
            source: e,
            msg: format!("sync {:?}", rand_name),
        })?;
        debug!("write_config: synced");

        rename(&rand_name, &self.path)
            .await
            .map_err(|e| ShareError::IoError {
                source: e,
                msg: format!("rename {:?} to {:?}", rand_name, self.path),
            })?;

        debug!("write_config: renamed to {:?}", self.path);
        Ok(())
    }

    async fn write_config(&self, config: &SmbDropState) -> Result<(), ShareError> {
        if let Err(e) = config.check_consistent(&self.user_provider) {
            warn!("consistency check failed, continuing: {e}");
        }
        self.write_config_no_check(config).await?;
        Ok(())
    }
}

static PATH_RE: OnceCell<Regex> = OnceCell::const_new();

const SHARE_NAME_HUMAN_PART_MAX_LEN: usize = 47;

fn sanitize_share_name_human_part(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter_map(|character| match character {
            ' ' => Some('-'),
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => Some(character),
            _ => None,
        })
        .collect();

    if sanitized.is_empty() {
        "share".to_owned()
    } else {
        sanitized
    }
}

fn make_share_name(dest_path: &Path, share_id: &uuid::Uuid) -> String {
    let human_part = dest_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_share_name_human_part)
        .unwrap_or_else(|| "share".to_owned());
    let human_part: String = human_part
        .chars()
        .take(SHARE_NAME_HUMAN_PART_MAX_LEN)
        .collect();

    format!("{human_part}-{}", share_id.simple())
}

async fn get_path_re() -> &'static Regex {
    PATH_RE
        .get_or_init(|| async {
            #[allow(clippy::unwrap_used)]
            Regex::new(r"^[-/a-zA-Z0-9_. ]*$").unwrap() // statically correct..?
        })
        .await
}

#[derive(Clone, Debug)]
pub struct ShareManager<U: UserProvider, M: MountBackend, P: ProcessManager> {
    config_file: Arc<Mutex<ShareConfigFile<U>>>,
    sm: SambaConfigManager,
    bind_mount_prefix: PathBuf,
    pm_tx: Option<Sender<ProcessManagerCommand>>,
    mount_backend: Arc<M>,
    user_provider: U,
    process_manager: P,
}

impl<U: UserProvider, M: MountBackend + 'static, P: ProcessManager> ShareManager<U, M, P> {
    pub async fn new(
        bind_mount_prefix: PathBuf,
        shares_config: PathBuf,
        samba_config_path: PathBuf,
        user_provider: U,
        mount_backend: Arc<M>,
        process_manager: P,
    ) -> Self {
        let config_file = Arc::new(Mutex::new(ShareConfigFile::new(
            shares_config,
            user_provider.clone(),
        )));
        let sm = SambaConfigManager::new(&samba_config_path, &bind_mount_prefix);

        Self {
            config_file,
            pm_tx: None,
            sm,
            bind_mount_prefix,
            mount_backend,
            user_provider,
            process_manager,
        }
    }

    fn mm(&self) -> MountManager {
        MountManager::new(&self.bind_mount_prefix, Arc::clone(&self.mount_backend))
    }

    pub async fn start(&mut self) -> Result<(), ShareError> {
        // ensure mounts and samba config are up-to-date before starting samba:
        let config_file = self.config_file.lock().await;
        let state = config_file.read_config_or_default().await?;

        let failed_mounts: HashSet<BindMount> = match self.update_mounts(&state).await {
            Ok(_) => HashSet::default(),
            Err(SetStateError::Reconciliation(failures)) => {
                failures.failed_mounts.into_iter().collect()
            }
            Err(e) => {
                return Err(ShareError::StateError(e));
            }
        };

        self.update_samba(&state, &failed_mounts).await?;
        self.ensure_users(&state).await?;

        let pm_tx = self.process_manager.start_in_background(false).await;
        self.pm_tx = Some(pm_tx);

        Ok(())
    }

    pub async fn ensure_users(&self, config: &SmbDropState) -> Result<(), ShareError> {
        for user in config.users.values() {
            let pwd =
                self.user_provider
                    .passwd_from_uid(user.uid)
                    .map_err(|e| ShareError::IoError {
                        source: e,
                        msg: format!("lookup uid {} in ensure_users", user.uid),
                    })?;
            if pwd.is_none() {
                self.add_system_user(user).await?;
            }
        }

        Ok(())
    }

    pub async fn reload(&self) -> Result<(), ShareError> {
        if let Some(ref pm_tx) = self.pm_tx {
            pm_tx
                .send(ProcessManagerCommand::Reload)
                .await
                .map_err(|e| ShareError::ProcessManagerError(e.to_string()))?;
            Ok(())
        } else {
            Err(ShareError::ProcessManagerError(
                "process manager not running".to_string(),
            ))
        }
    }

    pub async fn stop(&self) -> Result<(), ShareError> {
        if let Some(ref pm_tx) = self.pm_tx {
            pm_tx
                .send(ProcessManagerCommand::Stop)
                .await
                .map_err(|e| ShareError::ProcessManagerError(e.to_string()))?;
            Ok(())
        } else {
            Err(ShareError::ProcessManagerError(
                "process manager not running".to_string(),
            ))
        }
    }

    /// Returns a map of share IDs to shares
    pub async fn list_shares(&self) -> Result<HashMap<String, Share>, ShareError> {
        let config_file = self.config_file.lock().await;
        Ok(config_file.read_config_or_default().await?.shares)
    }

    pub async fn get_state(&self) -> Result<SmbDropState, ShareError> {
        let config_file = self.config_file.lock().await;
        config_file.read_config_or_default().await
    }

    /// Manipulate the configuration while holding the config file lock,
    /// hopefully preventing concurrent accessed that could result in
    /// inconsistent states. You pass in an async closure, which may read
    /// and write to the `ShareConfigFile` which it is given.
    pub async fn with_locked_config<R>(
        &self,
        fun: impl AsyncFnOnce(&ShareConfigFile<U>) -> R,
    ) -> Result<R, ShareError> {
        let config_file = self.config_file.lock().await;
        Ok(fun(&config_file).await)
    }

    /// Create a new share with the given parameters.
    ///
    /// This is a high-level function that will take care of the whole process
    /// of adding a new share:
    ///
    /// - generate a share ID, username and password
    /// - read/modify/write share configuration file
    /// - manage system and samba users
    /// - create bind mounts that ensure we are sharing by directory identity
    ///   (inode), and not by path (which can be renamed over time)
    pub async fn create_share(
        &self,
        dest_path: &Path,
        uid: u32,
        gid: u32,
        resource: Option<String>,
        created_by: Option<String>,
    ) -> Result<Share, ShareError> {
        // before touching the config, try to heal any inconsistencies:
        self.try_heal().await?;

        let pat = get_path_re().await;
        info!("creating share to {}", dest_path.to_string_lossy());

        let dest_path = if let Some(path) = dest_path.to_str() {
            path
        } else {
            return Err(ShareError::PathError);
        };
        if !pat.is_match(dest_path) {
            return Err(ShareError::PathError);
        }

        let share_id = uuid::Uuid::new_v4();
        let share_name = make_share_name(Path::new(dest_path), &share_id);
        let share_id = share_id.hyphenated().to_string();

        let config_file = self.config_file.lock().await;

        debug!("create_share: reading config...");
        let mut state = if config_file.path.exists() {
            config_file.read_config_or_default().await?
        } else {
            SmbDropState::empty()
        };
        debug!("create_share: config read");

        let user = state.get_or_create_user(uid, share_id.clone()).await?;

        debug!("create_share: user {user:?} created in config, if it didn't exist");

        let share = Share {
            resource,
            share_id: share_id.clone(),
            share_name,
            dest_path: dest_path.to_owned(),
            username: user.username.clone(),
            password: user.password.clone(),
            uid,
            gid,
            created_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| ShareError::ClockIssues)?
                .as_secs(),
            created_by,
        };

        // before writing the config, run the pre-flight checks (without immutability)
        // so we can be confident the mount will succeed later
        self.share_preflight_checks(&share).await?;

        debug!("create_share: preflight successful, adding system user");

        self.add_system_user(&user).await?;

        // attaching is part of creating; if re-attaching existing shares is ever needed, move
        // the resource assignment into its own state transition that calls this as well
        if let Some(resource) = &share.resource {
            state.detach_other_uids(resource, uid);
        }
        state.add_share(share.clone()).await?;
        debug!("create_share: share {share_id} added");

        config_file.write_config(&state).await?;

        debug!("create_share: config written");

        debug!("create_share: updating mounts...");
        let failed_mounts: HashSet<BindMount> = match self.update_mounts(&state).await {
            Ok(_) => HashSet::default(),
            Err(SetStateError::Reconciliation(failures)) => {
                failures.failed_mounts.into_iter().collect()
            }
            Err(e) => {
                return Err(ShareError::StateError(e));
            }
        };
        debug!("create_share: updating samba...");
        self.update_samba(&state, &failed_mounts).await?;
        self.reload().await?;

        let this_mount = BindMount::from_share(&share, self.mm().get_prefix());
        if failed_mounts.contains(&this_mount) {
            return Err(ShareError::OtherMountError);
        }

        debug!("create_share: done!");
        info!("created share {} to {}", share.share_id, share.dest_path);
        Ok(share)
    }

    pub async fn delete_share(&self, share_id: &str) -> Result<(), ShareError> {
        // before touching the config, try to heal any inconsistencies:
        self.try_heal().await?;
        let config_file = self.config_file.lock().await;
        let mut state = config_file.read_config_or_default().await?;
        let removed_share = state.remove_share(share_id).await?;

        self.update_samba(&state, &HashSet::default()).await?;
        self.reload().await?;
        let failed_mounts: HashSet<BindMount> = match self.update_mounts(&state).await {
            Ok(()) => HashSet::default(),
            Err(SetStateError::Reconciliation(failures)) => {
                warn!(
                    "continuing removal of share {share_id} after mount reconciliation failed: {failures}"
                );
                failures.failed_mounts.into_iter().collect()
            }
            Err(error) => return Err(ShareError::StateError(error)),
        };

        // The first update withdraws the removed share before unmounting. If
        // reconciliation also found unavailable remaining shares, update the
        // Samba configuration once more so they are not exported either.
        if !failed_mounts.is_empty() {
            self.update_samba(&state, &failed_mounts).await?;
            self.reload().await?;
        }

        // if the number of shares with a matching `uid` is now zero, we can remove the user:
        let share_count = state
            .shares
            .values()
            .filter(|s| s.uid == removed_share.uid)
            .count();
        if share_count == 0 {
            state.remove_user(removed_share.uid);
            self.remove_system_user(&removed_share.username).await?;
        }

        // FIXME: is it ok to delay writing the updated config until here?
        // if we don't delay, we may write an "inconsistent" intermediate version,
        // where unused users aren't removed yet
        config_file.write_config(&state).await?;

        info!("removed share {}", share_id);

        Ok(())
    }

    /// Persist `user` into the system user database, and create a matching
    /// samba user.
    async fn add_system_user(&self, user: &User) -> Result<(), ShareError> {
        let pwd =
            self.user_provider
                .passwd_from_uid(user.uid)
                .map_err(|e| ShareError::IoError {
                    source: e,
                    msg: format!("look up uid {} in add_system_user", user.uid),
                })?;
        if pwd.is_some() {
            info!("system user {} already exists", user.username);
            return Ok(());
        }

        self.user_provider.add_system_user(user).await?;
        self.user_provider.add_samba_user(user).await?;

        Ok(())
    }

    /// Remove both system and samba user matching `username`.
    async fn remove_system_user(&self, username: &str) -> Result<(), ShareError> {
        self.user_provider.remove_samba_user(username).await?;
        self.user_provider.remove_system_user(username).await?;

        Ok(())
    }

    /// Before trying to mount, run some preflight checks.
    async fn share_preflight_checks(&self, share: &Share) -> Result<(), ShareError> {
        let mount = BindMount::from_share(share, self.mm().get_prefix());
        self.mm().preflight(&mount)?;

        Ok(())
    }

    async fn update_mounts(&self, config: &SmbDropState) -> Result<(), SetStateError> {
        let desired_mounts: HashSet<BindMount> = config
            .shares
            .values()
            .map(|share| BindMount::from_share(share, self.mm().get_prefix()))
            .collect();
        self.mm().set_state(desired_mounts)?;

        Ok(())
    }

    async fn update_samba(
        &self,
        config: &SmbDropState,
        failed_mounts: &HashSet<BindMount>,
    ) -> Result<(), ShareError> {
        let failed_mount_points: HashSet<PathBuf> = failed_mounts
            .iter()
            .cloned()
            .map(|mount| mount.mount_point)
            .collect();
        let shares = config.shares.values().filter(|share| {
            let mount = BindMount::from_share(share, self.mm().get_prefix());
            !failed_mount_points.contains(&mount.mount_point)
        });
        self.sm.update_shares(shares).await?;
        Ok(())
    }

    async fn try_cleanup_user(&self, state: &mut SmbDropState, uid: u32) -> Result<(), ShareError> {
        let user = state.users.get(&uid.to_string()).cloned().ok_or_else(|| {
            ShareError::Inconsistent(format!(
                "try_heal: could not look up user by uid {uid} - mismatch str key vs. User::uid?"
            ))
        })?;
        state.remove_user(uid);
        self.remove_system_user(&user.username).await?;
        Ok(())
    }

    /// Try to "heal" the configuration - if there are inconsistencies,
    /// try to resolve them. For example, if users exist that are not used
    /// by any shares.
    ///
    /// Errors that happen while trying to heal are logged, but don't cause the
    /// healing process to abort, as we want to eventually succeed, and some operations
    /// might fail because of the inconsistent state (example: a user is present only
    /// in the configuration, but not as a system user - we want to try to remove
    /// the user from both places in any case, and not abort, as we want to try to
    /// continue with the next user, if there is any). Errors from reading/writing
    /// the configuration file do cause a hard failure though.
    pub async fn try_heal(&self) -> Result<(), ShareError> {
        self.with_locked_config(async |config_file| -> Result<(), ShareError> {
            let mut state = config_file.read_config_or_default().await?;
            let share_uids: HashSet<u32> = state.shares.values().map(|s| s.uid).collect();
            let user_uids: HashSet<u32> = state.users.values().map(|u| u.uid).collect();
            let unused_users = &user_uids - &share_uids;
            debug!("unused_users: {unused_users:?}");
            for uid in &unused_users {
                warn!("cleaning up uid {uid}");
                if let Err(e) = self.try_cleanup_user(&mut state, *uid).await {
                    error!("cleanup for uid {uid} failed: {e}");
                }
            }

            // remove any system users that are in an inconsistent state,
            // they are re-added afterwards
            for (uid, user) in state.users.iter().sorted() {
                if let Ok(Some(sys_pwd)) =
                    self.user_provider.passwd_from_name_string(&user.username)
                    && sys_pwd.uid != user.uid
                    && let Err(e) = self.remove_system_user(&user.username).await
                {
                    error!("cleanup for uid {uid} failed: {e}");
                }
            }

            config_file.write_config(&state).await?;

            // re-add any system users that we may have removed above:
            self.ensure_users(&state).await?;

            Ok(())
        })
        .await??;

        // TODO: re-create missing samba users
        // this is not trivial, as we need to check if a samba user already exists
        // for system users, this is already done in `ensure_users`.

        // TODO: on username mismatch, update username on share
        // TODO: see additional checks in `check_consistency` and how we can heal them

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{collections::HashMap, path::Path, sync::Arc};

    use itertools::Itertools as _;
    use log::debug;
    use modama_common::{logging::init_logging, messages::Share, test_helpers::TempDir};

    use crate::{
        share_manager::{
            ShareConfigFile, ShareError, ShareManager, SmbDropState, make_share_name,
            sanitize_share_name_human_part,
        },
        test_helpers::{MockMountBackend, MockProcessManager, MockUserProvider},
    };

    #[test]
    fn test_sanitize_share_name_human_part() {
        assert_eq!(
            sanitize_share_name_human_part("Session 01-A_b"),
            "Session-01-A_b"
        );
        assert_eq!(sanitize_share_name_human_part("Session  01"), "Session--01");
        assert_eq!(sanitize_share_name_human_part("résumé.v1\t[x]"), "rsumv1x");
        assert_eq!(sanitize_share_name_human_part("\n\r\t.é"), "share");
    }

    #[test]
    fn test_make_share_name() {
        let share_id = uuid::Uuid::parse_str("123e4567-e89b-42d3-a456-426614174000").unwrap();

        assert_eq!(
            make_share_name(Path::new("/adhoc/project/My Session/"), &share_id),
            "My-Session-123e4567e89b42d3a456426614174000"
        );
        assert_eq!(
            make_share_name(Path::new("/"), &share_id),
            "share-123e4567e89b42d3a456426614174000"
        );
    }

    #[test]
    fn test_make_share_name_truncates_to_windows_limit() {
        let share_id = uuid::Uuid::parse_str("123e4567-e89b-42d3-a456-426614174000").unwrap();
        let path = format!("/adhoc/project/{}/", "A".repeat(48));
        let share_name = make_share_name(Path::new(&path), &share_id);

        assert_eq!(share_name.len(), 80);
        assert_eq!(
            share_name,
            format!("{}-123e4567e89b42d3a456426614174000", "A".repeat(47))
        );
        assert!(
            share_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
    }

    /// Agents and session helpers match share names case-insensitively (SMB share names are),
    /// so sessions whose names differ only in case must still get names that differ beyond case
    #[test]
    fn test_make_share_name_differs_beyond_case_for_sessions_differing_in_case() {
        let upper_id = uuid::Uuid::parse_str("123e4567-e89b-42d3-a456-426614174000").unwrap();
        let lower_id = uuid::Uuid::parse_str("9f1c0d2a-b7e4-4c1e-8a0b-5d3e6f7a8b9c").unwrap();
        let upper = make_share_name(Path::new("/adhoc/project/2026-10-SQL/"), &upper_id);
        let lower = make_share_name(Path::new("/adhoc/project/2026-10-sql/"), &lower_id);

        assert_eq!(upper, "2026-10-SQL-123e4567e89b42d3a456426614174000");
        assert_eq!(lower, "2026-10-sql-9f1c0d2ab7e44c1e8a0b5d3e6f7a8b9c");
        assert_ne!(upper.to_lowercase(), lower.to_lowercase());
    }

    #[tokio::test]
    async fn test_smbdrop_state_get_or_create_user() {
        let shares = HashMap::new();
        let users = HashMap::new();
        let mut state = SmbDropState { shares, users };

        let share_id = uuid::Uuid::new_v4();
        let share_id = share_id.hyphenated().to_string();

        // same uid:
        let u1 = state
            .get_or_create_user(1001, share_id.clone())
            .await
            .unwrap();
        let u2 = state
            .get_or_create_user(1001, share_id.clone())
            .await
            .unwrap();
        assert_eq!(u1, u2);

        // different uid, different share id:
        let share_id_2 = uuid::Uuid::new_v4();
        let share_id_2 = share_id_2.hyphenated().to_string();
        let u3 = state
            .get_or_create_user(1002, share_id_2.clone())
            .await
            .unwrap();
        assert_ne!(u1, u3);
    }

    #[tokio::test]
    async fn test_smbdrop_state_invalid_share_id() {
        let shares = HashMap::new();
        let users = HashMap::new();
        let mut state = SmbDropState { shares, users };

        assert!(matches!(
            state
                .get_or_create_user(1001, "something-unique".to_owned())
                .await,
            Err(ShareError::InvalidShareId(_))
        ));
    }

    #[tokio::test]
    async fn test_smbdrop_consistency() {
        let mut state = SmbDropState {
            shares: HashMap::new(),
            users: HashMap::new(),
        };
        let share_id = String::from("b4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let username = "shareb4fdcdf6e8704ac5994278463f";

        let share_id_2 = String::from("Y4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let username_2 = "shareY4fdcdf6e8704ac5994278463f";

        {
            let mut state = state.clone();
            let share = Share {
                resource: Some("res".to_owned()),
                share_id: share_id.clone(),
                share_name: share_id.clone(),
                dest_path: "/adhoc/the-project/test-session/".to_owned(),
                created_timestamp: 0,
                username: "different".to_owned(),
                password: "share-pass".to_owned(),
                uid: 1001,
                gid: 1001,
                created_by: None,
            };
            state.add_share(share.clone()).await.unwrap();
            state
                .get_or_create_user(1001, share_id.clone())
                .await
                .unwrap();
            let user_provider = MockUserProvider::new(vec![(username.to_owned(), 1001)]);
            match state.check_consistent(&user_provider) {
                Err(ShareError::Inconsistent(m)) => {
                    assert_eq!(
                        m,
                        "username mismatch for 1001: user entry `shareb4fdcdf6e8704ac5994278463f` vs share `different`".to_owned()
                    )
                }
                res => {
                    panic!("unexpected result: {res:?}");
                }
            };
        }

        {
            let mut state = state.clone();
            let share = Share {
                resource: Some("res".to_owned()),
                share_id: share_id.clone(),
                share_name: share_id.clone(),
                dest_path: "/adhoc/the-project/test-session/".to_owned(),
                created_timestamp: 0,
                username: username.to_owned(),
                password: "share-pass".to_owned(),
                uid: 1002, // XXX uid mismatch
                gid: 1002,
                created_by: None,
            };
            state.add_share(share.clone()).await.unwrap();
            state
                .get_or_create_user(1001, share_id.clone())
                .await
                .unwrap();
            let user_provider = MockUserProvider::new(vec![(username.to_owned(), 1001)]);
            match state.check_consistent(&user_provider) {
                Err(ShareError::Inconsistent(m)) => {
                    assert_eq!(
                        m,
                        "mismatch in UIDs of shares ([1002]) and users ([1001])".to_owned()
                    )
                }
                res => {
                    panic!("unexpected result: {res:?}");
                }
            };
        }

        {
            let mut state = state.clone();
            let share = Share {
                resource: Some("res".to_owned()),
                share_id: share_id.clone(),
                share_name: share_id.clone(),
                dest_path: "/adhoc/the-project/test-session/".to_owned(),
                created_timestamp: 0,
                username: username.to_owned(),
                password: "share-pass".to_owned(),
                uid: 1001,
                gid: 1001,
                created_by: None,
            };
            state.add_share(share.clone()).await.unwrap();
            // uid mismatch here:
            state
                .get_or_create_user(1002, share_id.clone())
                .await
                .unwrap();
            let user_provider = MockUserProvider::new(vec![(username.to_owned(), 1001)]);
            match state.check_consistent(&user_provider) {
                Err(ShareError::Inconsistent(m)) => {
                    assert_eq!(
                        m,
                        "mismatch in UIDs of shares ([1001]) and users ([1002])".to_owned()
                    )
                }
                res => {
                    panic!("unexpected result: {res:?}");
                }
            };
        }

        // multiple shares, multiple uesrs, but with a uid mismatch:
        {
            let mut state = state.clone();
            let share = Share {
                resource: Some("res".to_owned()),
                share_id: share_id.clone(),
                share_name: share_id.clone(),
                dest_path: "/adhoc/the-project/test-session/".to_owned(),
                created_timestamp: 0,
                username: username.to_owned(),
                password: "share-pass".to_owned(),
                uid: 1001,
                gid: 1001,
                created_by: None,
            };
            state.add_share(share.clone()).await.unwrap();

            let share = Share {
                resource: Some("res".to_owned()),
                share_id: share_id_2.clone(),
                share_name: share_id_2.clone(),
                dest_path: "/adhoc/the-project/test-session/".to_owned(),
                created_timestamp: 0,
                username: username_2.to_owned(),
                password: "share-pass".to_owned(),
                uid: 1002,
                gid: 1002,
                created_by: None,
            };
            state.add_share(share.clone()).await.unwrap();

            // add the uesrs, but with a "crossover"
            state
                .get_or_create_user(1002, share_id.clone())
                .await
                .unwrap();
            state
                .get_or_create_user(1001, share_id_2.clone())
                .await
                .unwrap();

            let user_provider = MockUserProvider::new(vec![
                (username.to_owned(), 1001),
                (username_2.to_owned(), 1002),
            ]);
            match state.check_consistent(&user_provider) {
                Err(ShareError::Inconsistent(m)) => {
                    assert_eq!(
                        m,
                        "uid mismatch: 1002 (system) vs 1001 (share config)".to_owned()
                    )
                }
                res => {
                    panic!("unexpected result: {res:?}");
                }
            };
        }

        let share = Share {
            resource: Some("res".to_owned()),
            share_id: share_id.clone(),
            share_name: share_id.clone(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            created_timestamp: 0,
            username: username.to_owned(),
            password: "share-pass".to_owned(),
            uid: 1001,
            gid: 1001,
            created_by: None,
        };

        // adding a share without a user:
        state.add_share(share.clone()).await.unwrap();
        let user_provider = MockUserProvider::new(vec![(username.to_owned(), 1001)]);
        assert!(matches!(
            state.check_consistent(&user_provider),
            Err(ShareError::Inconsistent(_)),
        ));

        {
            // if we add the wrong user...
            let mut state = state.clone();

            state
                .get_or_create_user(1001, "XXXXXdf6-e870-4ac5-9942-78463f051c5c".to_owned())
                .await
                .unwrap();
            let user_provider = MockUserProvider::new(vec![(username.to_owned(), 1001)]);
            match state.check_consistent(&user_provider) {
                Err(ShareError::Inconsistent(m)) => {
                    assert_eq!(
                        m,
                        "user `shareXXXXXdf6e8704ac5994278463f` not found".to_owned()
                    )
                }
                res => {
                    panic!("unexpected result: {res:?}");
                }
            };
        }

        // fix by adding a matching user:
        state
            .get_or_create_user(1001, share_id.clone())
            .await
            .unwrap();
        let user_provider = MockUserProvider::new(vec![(username.to_owned(), 1001)]);
        state.check_consistent(&user_provider).unwrap();
    }

    fn resource_share(share_id: &str, uid: u32, resource: Option<&str>) -> Share {
        Share {
            resource: resource.map(str::to_owned),
            share_id: share_id.to_owned(),
            share_name: share_id.to_owned(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            created_timestamp: 0,
            username: format!("user{uid}"),
            password: "share-pass".to_owned(),
            uid,
            gid: uid,
            created_by: None,
        }
    }

    #[tokio::test]
    async fn test_detach_other_uids() {
        let mut state = SmbDropState::empty();
        for share in [
            resource_share("a", 1001, Some("res")),
            resource_share("b", 1002, Some("res")),
            resource_share("c", 1001, Some("other")),
            resource_share("d", 1001, None),
        ] {
            state.add_share(share).await.unwrap();
        }

        state.detach_other_uids("res", 1002);

        let resources: Vec<(&str, Option<&str>)> = state
            .shares
            .values()
            .sorted_by_key(|s| &s.share_id)
            .map(|s| (s.share_id.as_str(), s.resource.as_deref()))
            .collect();
        assert_eq!(
            resources,
            vec![
                ("a", None),
                ("b", Some("res")),
                ("c", Some("other")),
                ("d", None),
            ]
        );
    }

    #[tokio::test]
    async fn test_create_share_detaches_other_uids() {
        let tmp = TempDir::new("share_config_file");
        let dest_path = TempDir::new("dest_path_for_share");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");
        let mut share_manager = ShareManager::new(
            bind_mount_prefix.to_owned(),
            tmp.join("shares.json"),
            tmp.join("smb_dynamic.conf"),
            MockUserProvider::new(vec![]),
            Arc::new(MockMountBackend::new()),
            MockProcessManager::new(),
        )
        .await;
        share_manager.start().await.unwrap();
        let create = |uid: u32| {
            share_manager.create_share(
                &dest_path,
                uid,
                uid,
                Some("holo".to_owned()),
                Some(format!("user{uid}")),
            )
        };

        let first = create(1001).await.unwrap();
        let second = create(1002).await.unwrap();
        let third = create(1002).await.unwrap();

        let state = share_manager.get_state().await.unwrap();
        let resource = |share: &Share| state.shares[&share.share_id].resource.clone();
        // the previous user's share is detached, but still there:
        assert_eq!(resource(&first), None);
        // shares of the same uid stay attached together:
        assert_eq!(resource(&second), Some("holo".to_owned()));
        assert_eq!(resource(&third), Some("holo".to_owned()));
    }

    #[tokio::test]
    async fn test_consistency_one_uid_per_resource() {
        // two uids with matching users, so only the resource check can fail:
        let mut state = SmbDropState::empty();
        let mut known_users = vec![];
        for (share_id, uid) in [
            ("b4fdcdf6-e870-4ac5-9942-78463f051c5c", 1001),
            ("c4fdcdf6-e870-4ac5-9942-78463f051c5c", 1002),
        ] {
            let user = state
                .get_or_create_user(uid, share_id.to_owned())
                .await
                .unwrap();
            let mut share = resource_share(share_id, uid, Some("res"));
            share.username = user.username.clone();
            state.add_share(share).await.unwrap();
            known_users.push((user.username, uid));
        }
        let user_provider = MockUserProvider::new(known_users);

        match state.check_consistent(&user_provider) {
            Err(ShareError::Inconsistent(m)) => {
                assert_eq!(m, "resource `res` has shares of uids 1001 and 1002")
            }
            res => panic!("unexpected result: {res:?}"),
        }

        // on different resources, or detached, it's fine:
        for resource in [Some("other"), None] {
            let mut state = state.clone();
            state
                .shares
                .get_mut("c4fdcdf6-e870-4ac5-9942-78463f051c5c")
                .unwrap()
                .resource = resource.map(str::to_owned);
            state.check_consistent(&user_provider).unwrap();
        }
    }

    #[tokio::test]
    async fn test_heal_inconsistent_uids() {
        init_logging();

        // same setup as `test_smbdrop_consistency` for the inner state,
        // but we also need to have a `ShareManager`:
        let state = SmbDropState {
            shares: HashMap::new(),
            users: HashMap::new(),
        };
        let share_id = String::from("b4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let username = "shareb4fdcdf6e8704ac5994278463f";

        let share_id_2 = String::from("Y4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let username_2 = "shareY4fdcdf6e8704ac5994278463f";

        let configs_path = TempDir::new("share_manager_configs");
        let bind_mount_prefix = TempDir::new("bind_mount_prefix");

        let user_provider = MockUserProvider::new(vec![
            (username.to_owned(), 1001),
            (username_2.to_owned(), 1002),
        ]);
        let mount_backend: Arc<MockMountBackend> = Arc::new(MockMountBackend::new());
        let process_manager: MockProcessManager = MockProcessManager::new();

        let share_manager = ShareManager::new(
            bind_mount_prefix.to_owned(),
            configs_path.join("shares_config.json"),
            configs_path.join("smb_dynamic.conf"),
            user_provider.clone(),
            mount_backend,
            process_manager.clone(),
        )
        .await;

        {
            let mut state = state.clone();
            let share = Share {
                resource: Some("res".to_owned()),
                share_id: share_id.clone(),
                share_name: share_id.clone(),
                dest_path: "/adhoc/the-project/test-session/".to_owned(),
                created_timestamp: 0,
                username: username.to_owned(),
                password: "share-pass".to_owned(),
                // this uid exists, but there's an additional one in the users key of the config:
                uid: 1001,
                gid: 1001,
                created_by: None,
            };
            state.add_share(share.clone()).await.unwrap();
            state
                .get_or_create_user(1001, share_id.clone())
                .await
                .unwrap();
            // XXX one additional user here:
            state
                .get_or_create_user(1002, share_id_2.clone())
                .await
                .unwrap();

            share_manager
                .with_locked_config(async |config_file| {
                    config_file.write_config_no_check(&state).await.unwrap();
                })
                .await
                .unwrap();

            match state.check_consistent(&user_provider) {
                Err(ShareError::Inconsistent(m)) => {
                    assert_eq!(
                        m,
                        "mismatch in UIDs of shares ([1001]) and users ([1001, 1002])".to_owned()
                    )
                }
                res => {
                    panic!("unexpected result: {res:?}");
                }
            };

            let users_snapshot = {
                let users = user_provider.users.lock().unwrap();
                users.clone()
            };
            debug!("{users_snapshot:?}");
            assert_eq!(
                users_snapshot,
                vec![(username.to_owned(), 1001), (username_2.to_owned(), 1002),]
            );

            // now let's try to heal:
            share_manager.try_heal().await.unwrap();
            let new_state = share_manager.get_state().await.unwrap();
            debug!("s {:?}", state.shares);
            debug!("ns {:?}", new_state.shares);

            debug!("u {:?}", state.users);
            debug!("nu {:?}", new_state.users);
            new_state
                .check_consistent(&user_provider)
                .expect("should no longer return an error");

            assert_eq!(new_state.shares, state.shares);

            // user is gone from the config:
            let new_uids: Vec<String> = new_state.users.keys().cloned().collect();
            assert_eq!(new_uids, vec!["1001".to_owned()]);

            // user is gone from "the system" (which we mock here):
            let users_snapshot = {
                let users = user_provider.users.lock().unwrap();
                users.clone()
            };
            debug!("{users_snapshot:?}");
            assert_eq!(users_snapshot, vec![(username.to_owned(), 1001)]);
        }
    }

    #[tokio::test]
    async fn test_share_config_file() {
        let tmp = TempDir::new("share_config_file");
        let cfg_path = tmp.join("shares.json");
        let share_id = String::from("b4fdcdf6-e870-4ac5-9942-78463f051c5c");
        let username = "shareb4fdcdf6e8704ac5994278463f";
        let user_provider = MockUserProvider::new(vec![(username.to_owned(), 1001)]);
        let f = ShareConfigFile::new(cfg_path.clone(), user_provider.clone());

        let shares = HashMap::new();
        let users = HashMap::new();
        let mut state = SmbDropState { shares, users };

        let share = Share {
            resource: Some("res".to_owned()),
            share_id: share_id.clone(),
            share_name: share_id.clone(),
            dest_path: "/adhoc/the-project/test-session/".to_owned(),
            created_timestamp: 0,
            username: username.to_owned(),
            password: "share-pass".to_owned(),
            uid: 1001,
            gid: 1001,
            created_by: None,
        };

        state.add_share(share.clone()).await.unwrap();
        state
            .get_or_create_user(1001, share_id.clone())
            .await
            .unwrap();

        f.write_config(&state).await.unwrap();

        let f2 = ShareConfigFile::new(cfg_path.clone(), user_provider.clone());
        let state2 = f2.read_config().await.unwrap();

        assert_eq!(state, state2);
    }

    #[tokio::test]
    async fn test_heal_without_existing_config() {
        let tmp = TempDir::new("share_config_file");
        let cfg_path = tmp.join("shares.json");
        let user_provider = MockUserProvider::new(vec![]);

        let bind_mount_prefix = TempDir::new("bind_mount_prefix");
        let mount_backend: Arc<MockMountBackend> = Arc::new(MockMountBackend::new());
        let process_manager: MockProcessManager = MockProcessManager::new();
        let share_manager = ShareManager::new(
            bind_mount_prefix.to_owned(),
            cfg_path,
            tmp.join("smb_dynamic.conf"),
            user_provider.clone(),
            mount_backend,
            process_manager.clone(),
        )
        .await;

        share_manager.try_heal().await.unwrap();
    }
}
