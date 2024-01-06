use rand::{distributions::Alphanumeric, Rng};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    fs::{rename, File},
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{
        mpsc::{error::SendError, Sender},
        Mutex, OnceCell,
    },
    task::{spawn_blocking, JoinError},
};

use crate::{
    mount_manager::{BindMount, CompoundMountError, MountManager, SetStateError},
    process_manager::{PmCommand, SambaProcessManager},
    samba_config::{SambaConfigManager, UpdateSharesError},
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Share {
    pub resource: Option<String>,
    pub share_id: String,
    pub share_name: String,
    pub dest_path: PathBuf,

    /// unix timestamp
    pub created_timestamp: u64,

    /// temporary username
    pub username: String,

    /// temporary password
    pub password: String,

    /// the underlying unix user for which the share is created
    pub uid: u32,

    /// the underlying unix group for which the share is created
    pub gid: u32,
}

impl std::fmt::Debug for Share {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Share")
            .field("share_id", &self.share_id)
            .field("share_name", &self.share_name)
            .field("dest_path", &self.dest_path)
            .field("created_timestamp", &self.created_timestamp)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .finish()
    }
}

impl Share {
    pub fn to_mount(&self, bind_mount_prefix: &Path) -> BindMount {
        let mount_point = bind_mount_prefix.join("shares").join(&self.share_id);
        BindMount {
            mount_source: self.dest_path.to_owned(),
            mount_point,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SmbDropState {
    shares: HashMap<String, Share>,
    users: HashMap<String, User>,
}

impl SmbDropState {
    // FIXME: right now, we check for these consistency errors before writing
    // the state to JSON and abort - we might want to instead try to "heal" the
    // configuration and try to continue, such that an error in one share cannot
    // influence other shares being created.
    pub fn check_consistent(&self) -> Result<(), ShareError> {
        let share_uids: HashSet<u32> = self.shares.values().map(|s| s.uid).collect();
        let user_uids: HashSet<u32> = self.users.values().map(|u| u.uid).collect();
        if share_uids != user_uids {
            return Err(ShareError::Inconsistent(format!(
                "mismatch in UIDs of shares ({share_uids:?}) and users ({user_uids:?})"
            )));
        }

        for (_uid, user) in self.users.iter() {
            let c_username = if let Ok(c_username) = CString::new(user.username.as_bytes()) {
                c_username
            } else {
                return Err(ShareError::Inconsistent(format!(
                    "invalid username: `{}`",
                    &user.username
                )));
            };
            let pwd = etc_passwd::Passwd::from_name(&c_username)?;
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

            let pwd = etc_passwd::Passwd::from_uid(user.uid)?;
            if let Some(pwd) = pwd {
                let pwd_name = pwd.name.to_string_lossy();
                if pwd_name != user.username {
                    return Err(ShareError::Inconsistent(format!(
                        "username misatch for {}: `{}` (system) vs `{}` (share config)",
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
}

#[derive(thiserror::Error, Debug)]
pub enum ShareError {
    #[error("I/O error")]
    IoError(#[from] io::Error),

    #[error("serialization error")]
    SerdeError(#[from] serde_json::Error),

    #[error("background task failed")]
    JoinError(#[from] JoinError),

    #[error("Path name can only contain characters /, a-z, A-Z, 0-9, '_', '-', ' ' and '.'.")]
    PathError,

    #[error("share `{0}` not found")]
    ShareNotFound(String),

    #[error("inconcsistent configuration: {0}")]
    Inconsistent(String),

    #[error("could not sync mount state")]
    StateError(#[from] SetStateError),

    #[error("could not update shares")]
    UpdateShares(#[from] UpdateSharesError),

    #[error("could not reload samba")]
    SambaError(#[from] SendError<PmCommand>),
}

/// Return a path to a randomized temporariy filename in the same directory as
/// the file pointed to by the given `path`.
pub fn make_temporary_fname(path: &Path) -> PathBuf {
    let dirname = path
        .parent()
        .expect("config path must point to the configuration file");
    let filename = path
        .file_name()
        .expect("config path must point to the configuration file")
        .to_str()
        .expect("configuration path must be valid unicode");
    loop {
        let rnd: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(15)
            .map(char::from)
            .collect();
        let rand_name = dirname.join(format!("{}-tmp{}", filename, rnd));
        let path = dirname.join(rand_name);
        if !path.exists() {
            return path;
        }
    }
}

fn make_password() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                             abcdefghijklmnopqrstuvwxyz\
                             0123456789+/";
    const PASSWORD_LEN: usize = 32;
    let mut rng = rand::thread_rng();

    (0..PASSWORD_LEN)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Utility to manipulate on-disk configuration for smbdrop. Read-modify-write
/// cycles should be guarded by an external mutex.
struct ShareConfigFile {
    path: PathBuf,
}

impl ShareConfigFile {
    fn new(path: PathBuf) -> Self {
        Self { path }
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

    async fn write_config(&self, config: SmbDropState) -> Result<(), ShareError> {
        config.check_consistent()?;
        let serialized =
            spawn_blocking(move || -> Result<_, serde_json::Error> { serde_json::to_vec(&config) })
                .await??;

        let rand_name = make_temporary_fname(&self.path);
        let mut opts = File::options();
        opts.mode(0o600).create_new(true).write(true);
        let mut f = opts.open(&rand_name).await?;

        f.write_all(&serialized).await?;
        f.sync_all().await?;

        rename(&rand_name, &self.path).await?;

        Ok(())
    }

    /// Get the User for the given uid, or create a new one with a random password.
    /// Note that this will not create a system user, which is done separately.
    async fn get_or_create_user(&self, uid: u32, share_id: String) -> Result<User, ShareError> {
        let mut config = self.read_config().await?;

        let key = uid.to_string();
        if let Some(user) = config.users.get(&key) {
            Ok((*user).clone())
        } else {
            let username = format!("share{share_id}").replace('-', "");
            let password = make_password();
            let user = User {
                uid,
                username,
                password,
            };
            config.users.insert(key, user.clone());
            self.write_config(config).await?;
            Ok(user)
        }
    }

    async fn add_share(&self, share: Share) -> Result<(), ShareError> {
        let mut config = self.read_config().await?;
        let key = share.share_id.clone();
        config.shares.insert(key, share);
        self.write_config(config).await?;
        Ok(())
    }

    async fn remove_share(&self, share_id: &str) -> Result<(), ShareError> {
        let mut config = self.read_config().await?;
        if config.shares.remove(share_id).is_none() {
            return Err(ShareError::ShareNotFound(share_id.to_owned()));
        }
        self.write_config(config).await?;
        Ok(())
    }

    async fn clean_shares(&self) -> Result<(), ShareError> {
        let config = self.read_config().await?;
        let mut new_config = config.clone();
        new_config
            .shares
            .retain(|_share_id, share| share.dest_path.is_dir() && !share.dest_path.is_symlink());

        if new_config != config {
            self.write_config(new_config).await?;
        }

        Ok(())
    }
}

static PATH_RE: OnceCell<Regex> = OnceCell::const_new();

async fn get_path_re() -> &'static Regex {
    PATH_RE
        .get_or_init(|| async { Regex::new(r"^[-/a-zA-Z0-9_. ]*$").unwrap() })
        .await
}

pub struct ShareManager {
    config_file: Mutex<ShareConfigFile>,
    sm: SambaConfigManager,
    mm: MountManager,
    pm_tx: Sender<PmCommand>,
}

impl ShareManager {
    pub async fn new(bind_mount_prefix: PathBuf, shares_config: PathBuf) -> Self {
        let pm_tx = SambaProcessManager::start_in_background().await;

        Self {
            config_file: Mutex::new(ShareConfigFile::new(shares_config)),
            pm_tx,
            mm: MountManager::new(&bind_mount_prefix),
            sm: SambaConfigManager::new(
                &PathBuf::from("/etc/samba/smb-dynamic.conf"),
                &bind_mount_prefix,
            ),
        }
    }

    pub async fn reload(&self) {
        self.pm_tx
            .send(PmCommand::Reload)
            .await
            .expect("process manager no longer running");
    }

    pub async fn stop(&self) {
        self.pm_tx
            .send(PmCommand::Stop)
            .await
            .expect("process manager no longer running");
    }

    pub async fn create_share(
        &self,
        dest_path: &Path,
        uid: u32,
        gid: u32,
        resource: Option<String>,
    ) -> Result<String, ShareError> {
        let pat = get_path_re().await;

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
        let user = config_file
            .get_or_create_user(uid, share_id.clone())
            .await?;

        let share = Share {
            resource,
            share_id: share_id.clone(),
            share_name: format!("share-{share_id}"),
            dest_path: PathBuf::from(dest_path),
            username: user.username,
            password: user.password,
            uid,
            gid,
            created_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("does not work before UNIX_EPOCH")
                .as_secs(),
        };

        config_file.add_share(share).await?;
        config_file.clean_shares().await?;
        let config = config_file.read_config().await?;
        let failed_mounts: HashSet<BindMount> = match self.update_mounts(&config).await {
            Ok(_) => HashSet::default(),
            Err(SetStateError::MountError(CompoundMountError::Error(mounts))) => {
                mounts.iter().cloned().collect()
            }
            Err(e @ SetStateError::ProcError(_)) => {
                return Err(ShareError::StateError(e));
            }
        };
        self.update_samba(&config, failed_mounts).await?;
        self.pm_tx.send(PmCommand::Reload).await?;

        Ok(share_id)
    }

    pub async fn delete_share(&self, share_id: &str) -> Result<(), ShareError> {
        let config_file = self.config_file.lock().await;
        config_file.remove_share(share_id).await?;
        let config = config_file.read_config().await?;
        self.update_samba(&config, HashSet::default()).await?;
        self.pm_tx.send(PmCommand::Reload).await?;
        self.update_mounts(&config).await?;

        Ok(())
    }

    async fn update_mounts(&self, config: &SmbDropState) -> Result<(), SetStateError> {
        let desired_mounts: HashSet<BindMount> = config
            .shares
            .values()
            .map(|share| share.to_mount(self.mm.get_prefix()))
            .collect();
        self.mm.set_state(desired_mounts)?;

        Ok(())
    }

    async fn update_samba(
        &self,
        config: &SmbDropState,
        failed_mounts: HashSet<BindMount>,
    ) -> Result<(), ShareError> {
        let failed_dest_paths: HashSet<PathBuf> =
            failed_mounts.into_iter().map(|m| m.mount_source).collect();
        let shares = config
            .shares
            .values()
            .filter(|s| !failed_dest_paths.contains(&s.dest_path));
        self.sm.update_shares(shares).await?;
        Ok(())
    }
}
