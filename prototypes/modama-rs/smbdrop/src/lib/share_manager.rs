use itertools::Itertools;
use log::{debug, error, info, warn};
use modama_common::messages::Share;
use rand::{distr::Alphanumeric, Rng};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    io,
    option::Option,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    fs::{rename, File},
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{
        mpsc::{error::SendError, Sender},
        Mutex, OnceCell,
    },
    task::{spawn_blocking, JoinError},
};

use crate::{
    mount::{BindMount, CompoundMountError, MountError},
    mount_manager::{LinuxMountBackend, MountManager, SetStateError},
    process_manager::{ProcessManagerCommand, SambaProcessManager},
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

pub trait UserProvider {
    fn passwd_from_name(&self, name: impl AsRef<CStr>) -> io::Result<Option<etc_passwd::Passwd>>;
    fn passwd_from_uid(&self, uid: u32) -> io::Result<Option<etc_passwd::Passwd>>;
}

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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SmbDropState {
    pub shares: HashMap<String, Share>,
    pub users: HashMap<String, User>,
}

impl SmbDropState {
    // FIXME: right now, we check for these consistency errors before writing
    // the state to JSON and abort - we might want to instead try to "heal" the
    // configuration and try to continue, such that an error in one share cannot
    // influence other shares being created or removed.
    pub fn check_consistent(&self, user_provider: &impl UserProvider) -> Result<(), ShareError> {
        let share_uids: HashSet<u32> = self.shares.values().map(|s| s.uid).collect();
        let user_uids: HashSet<u32> = self.users.values().map(|u| u.uid).collect();
        if share_uids != user_uids {
            return Err(ShareError::Inconsistent(format!(
                "mismatch in UIDs of shares ({share_uids:?}) and users ({user_uids:?})"
            )));
        }

        for (_uid, user) in self.users.iter().sorted() {
            let c_username = if let Ok(c_username) = CString::new(user.username.as_bytes()) {
                c_username
            } else {
                return Err(ShareError::Inconsistent(format!(
                    "invalid username: `{}`",
                    &user.username
                )));
            };
            let pwd = user_provider.passwd_from_name(&c_username)?;
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
                    &user.username
                )));
            }

            let pwd = user_provider.passwd_from_uid(user.uid)?;
            if let Some(pwd) = pwd {
                let pwd_name = pwd.name.to_string_lossy();
                if pwd_name != user.username {
                    return Err(ShareError::Inconsistent(format!(
                        "username mismatch for {}: `{}` (system) vs `{}` (share config)",
                        user.uid, &pwd_name, &user.username
                    )));
                }
            } else {
                return Err(ShareError::Inconsistent(format!(
                    "user for id {} not found",
                    user.uid
                )));
            }

            for (_share_id, share) in self.shares.iter() {
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

        Ok(())
    }

    /// Get the User for the given uid, or create a new one with a random password.
    /// Note that this will not create a system user, which is done separately.
    pub async fn get_or_create_user(
        &mut self,
        uid: u32,
        share_id: String,
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
            let password = make_password();
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
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),

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
struct ShareConfigFile<U: UserProvider> {
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

    async fn read_config(&self) -> Result<SmbDropState, ShareError> {
        let mut f = File::open(&self.path).await?;
        let mut contents = Vec::new();
        f.read_to_end(&mut contents).await?;

        let config = spawn_blocking(move || -> Result<_, serde_json::Error> {
            serde_json::from_slice(&contents)
        })
        .await??;

        Ok(config)
    }

    async fn write_config(&self, config: &SmbDropState) -> Result<(), ShareError> {
        config.check_consistent(&self.user_provider)?;
        debug!("write_config");
        let config = config.clone();

        let serialized =
            spawn_blocking(move || -> Result<_, serde_json::Error> { serde_json::to_vec(&config) })
                .await??;

        let rand_name = make_temporary_fname(&self.path)?;
        debug!("write_config: writing to {rand_name:?}");
        let mut opts = File::options();
        opts.mode(0o600).create_new(true).write(true);
        let mut f = opts.open(&rand_name).await?;

        f.write_all(&serialized).await?;
        debug!("write_config: wrote all...");
        f.sync_all().await?;
        debug!("write_config: synced");

        rename(&rand_name, &self.path).await?;

        debug!("write_config: renamed");

        Ok(())
    }
}

static PATH_RE: OnceCell<Regex> = OnceCell::const_new();

async fn get_path_re() -> &'static Regex {
    PATH_RE
        .get_or_init(|| async {
            #[allow(clippy::unwrap_used)]
            Regex::new(r"^[-/a-zA-Z0-9_. ]*$").unwrap() // statically correct..?
        })
        .await
}

#[derive(Clone, Debug)]
pub struct ShareManager<U: UserProvider> {
    config_file: Arc<Mutex<ShareConfigFile<U>>>,
    sm: SambaConfigManager,
    bind_mount_prefix: PathBuf,
    pm_tx: Option<Sender<ProcessManagerCommand>>,
}

impl<U: UserProvider> ShareManager<U> {
    pub async fn new(
        bind_mount_prefix: PathBuf,
        shares_config: PathBuf,
        samba_config_path: PathBuf,
        user_provider: U,
    ) -> Self {
        let config_file = Arc::new(Mutex::new(ShareConfigFile::new(
            shares_config,
            user_provider,
        )));
        let sm = SambaConfigManager::new(&samba_config_path, &bind_mount_prefix);

        Self {
            config_file,
            pm_tx: None,
            sm,
            bind_mount_prefix,
        }
    }

    fn mm(&self) -> MountManager {
        MountManager::new(&self.bind_mount_prefix, LinuxMountBackend::new())
    }

    pub async fn start(&mut self) -> Result<(), ShareError> {
        // ensure mounts and samba config are up-to-date before starting samba:
        let config_file = self.config_file.lock().await;
        let state = config_file.read_config().await?;

        let failed_mounts: HashSet<BindMount> = match self.update_mounts(&state).await {
            Ok(_) => HashSet::default(),
            Err(SetStateError::MountError(CompoundMountError::MultiFailure(mounts))) => {
                mounts.iter().cloned().collect()
            }
            Err(e @ SetStateError::BackendError(_)) => {
                return Err(ShareError::StateError(e));
            }
        };

        self.update_samba(&state, &failed_mounts).await?;
        self.ensure_users(&state).await?;

        let pm_tx = SambaProcessManager::start_in_background(false).await;
        self.pm_tx = Some(pm_tx);

        Ok(())
    }

    pub async fn ensure_users(&self, config: &SmbDropState) -> Result<(), ShareError> {
        for user in config.users.values() {
            let pwd = etc_passwd::Passwd::from_uid(user.uid)?;
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
        Ok(config_file.read_config().await?.shares)
    }

    pub async fn get_state(&self) -> Result<SmbDropState, ShareError> {
        let config_file = self.config_file.lock().await;
        config_file.read_config().await
    }

    pub async fn create_share(
        &self,
        dest_path: &Path,
        uid: u32,
        gid: u32,
        resource: Option<String>,
    ) -> Result<Share, ShareError> {
        let pat = get_path_re().await;
        info!("creating share to {}", &dest_path.to_string_lossy());

        let dest_path = if let Some(path) = dest_path.to_str() {
            path
        } else {
            return Err(ShareError::PathError);
        };
        if !pat.is_match(dest_path) {
            return Err(ShareError::PathError);
        }

        let share_id = uuid::Uuid::new_v4();
        let share_id = share_id.hyphenated().to_string();

        let config_file = self.config_file.lock().await;

        let mut state = config_file.read_config().await?;

        debug!("create_share: config read");

        let user = state.get_or_create_user(uid, share_id.clone()).await?;

        debug!("create_share: user {user:?} created in config, if it didn't exist");

        let share = Share {
            resource,
            share_id: share_id.clone(),
            share_name: format!("share-{share_id}"),
            dest_path: dest_path.to_owned(),
            username: user.username.clone(),
            password: user.password.clone(),
            uid,
            gid,
            created_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| ShareError::ClockIssues)?
                .as_secs(),
        };

        // before writing the config, run the pre-flight checks (without immutability)
        // so we can be confident the mount will succeed later
        self.share_preflight_checks(&share).await?;

        self.add_system_user(&user).await?;

        state.add_share(share.clone()).await?;
        debug!("create_share: share {share_id} added");

        config_file.write_config(&state).await?;

        debug!("create_share: config written");

        debug!("create_share: updating mounts...");
        let failed_mounts: HashSet<BindMount> = match self.update_mounts(&state).await {
            Ok(_) => HashSet::default(),
            Err(SetStateError::MountError(CompoundMountError::MultiFailure(mounts))) => {
                mounts.iter().cloned().collect()
            }
            Err(e @ SetStateError::BackendError(_)) => {
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
        info!("created share {} to {}", &share.share_id, &share.dest_path);
        Ok(share)
    }

    pub async fn delete_share(&self, share_id: &str) -> Result<(), ShareError> {
        let config_file = self.config_file.lock().await;
        let mut state = config_file.read_config().await?;
        let removed_share = state.remove_share(share_id).await?;

        self.update_samba(&state, &HashSet::default()).await?;
        self.reload().await?;
        self.update_mounts(&state).await?;

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

        info!("removed share {}", &share_id);

        Ok(())
    }

    async fn add_system_user(&self, user: &User) -> Result<(), ShareError> {
        let pwd = etc_passwd::Passwd::from_uid(user.uid)?;
        if pwd.is_some() {
            info!("system user {} already exists", &user.username);
            return Ok(());
        }
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
            warn!("useradd failed; stderr:\n {}", &stderr);
            let stdout = String::from_utf8_lossy(&useradd_res.stdout);
            debug!("stdout from useradd:\n {}", &stdout);
            return Err(ShareError::UsermgmtError {
                username: user.username.clone(),
                stderr: stderr.to_string(),
            });
        }

        let mut smbpasswd = Command::new("smbpasswd")
            .args(["-s", "-a", &user.username])
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdin = smbpasswd.stdin.take().ok_or(ShareError::SmbpasswdError)?;
        let smbpasswd_in: String = format!("{}\n{}\n", &user.password, &user.password);
        stdin.write_all(smbpasswd_in.as_bytes()).await?;
        let res = smbpasswd.wait_with_output().await?;
        if !res.status.success() {
            let stderr = String::from_utf8_lossy(&res.stderr);
            warn!("smbpasswd failed; stderr:\n {}", &stderr);
            let stdout = String::from_utf8_lossy(&res.stdout);
            debug!("stdout from smbpasswd:\n {}", &stdout);
            return Err(ShareError::UsermgmtError {
                username: user.username.clone(),
                stderr: stderr.to_string(),
            });
        }

        Ok(())
    }

    async fn remove_system_user(&self, username: &str) -> Result<(), ShareError> {
        let smbpasswd_res = Command::new("smbpasswd")
            .args(["-x", username])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await?;
        if !smbpasswd_res.status.success() {
            let stderr = String::from_utf8_lossy(&smbpasswd_res.stderr);
            warn!("smbpasswd failed; stderr:\n {}", &stderr);
            let stdout = String::from_utf8_lossy(&smbpasswd_res.stdout);
            debug!("stdout from smbpasswd:\n {}", &stdout);
            return Err(ShareError::UsermgmtError {
                username: username.to_string(),
                stderr: stderr.to_string(),
            });
        }

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
            warn!("userdel failed; stderr:\n {}", &stderr);
            let stdout = String::from_utf8_lossy(&userdel_res.stdout);
            debug!("stdout from userdel:\n {}", &stdout);
            return Err(ShareError::UsermgmtError {
                username: username.to_string(),
                stderr: stderr.to_string(),
            });
        }

        Ok(())
    }

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
        let failed_dest_paths: HashSet<PathBuf> = failed_mounts
            .iter()
            .cloned()
            .map(|m| m.mount_source)
            .collect();
        let shares = config
            .shares
            .values()
            .filter(|s| !failed_dest_paths.contains(&s.get_dest_path()));
        self.sm.update_shares(shares).await?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{
        collections::HashMap,
        ffi::{CStr, CString},
        io,
    };

    use etc_passwd::Passwd;
    use modama_common::{messages::Share, test_helpers::TempDir};

    use crate::share_manager::{ShareConfigFile, ShareError, SmbDropState, UserProvider};

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

    #[derive(Clone)]
    struct MockUserProvider {
        users: Vec<(String, u32)>,
    }

    impl MockUserProvider {
        fn new(users: Vec<(String, u32)>) -> Self {
            Self { users }
        }
    }

    impl UserProvider for MockUserProvider {
        fn passwd_from_name(
            &self,
            name: impl AsRef<CStr>,
        ) -> io::Result<Option<etc_passwd::Passwd>> {
            let name: &CStr = name.as_ref();

            for entry in self.users.iter() {
                if String::from_utf8(name.to_bytes().to_owned()).unwrap() == entry.0 {
                    return Ok(Some(Passwd {
                        name: CString::new(entry.0.as_bytes()).unwrap(),
                        passwd: CString::new("x".as_bytes()).unwrap(),
                        uid: entry.1,
                        gid: entry.1,
                        gecos: CString::new("".as_bytes()).unwrap(),
                        dir: CString::new("".as_bytes()).unwrap(),
                        shell: CString::new("".as_bytes()).unwrap(),
                    }));
                }
            }
            Ok(None)
        }

        fn passwd_from_uid(&self, uid: u32) -> io::Result<Option<etc_passwd::Passwd>> {
            for entry in self.users.iter() {
                if uid == entry.1 {
                    return Ok(Some(Passwd {
                        name: CString::new(entry.0.as_bytes()).unwrap(),
                        passwd: CString::new("x".as_bytes()).unwrap(),
                        uid: entry.1,
                        gid: entry.1,
                        gecos: CString::new("".as_bytes()).unwrap(),
                        dir: CString::new("".as_bytes()).unwrap(),
                        shell: CString::new("".as_bytes()).unwrap(),
                    }));
                }
            }
            Ok(None)
        }
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
                        "mismatch in UIDs of shares ({1002}) and users ({1001})".to_owned()
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
                        "mismatch in UIDs of shares ({1001}) and users ({1002})".to_owned()
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
}
