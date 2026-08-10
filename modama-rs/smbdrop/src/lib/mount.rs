use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use modama_common::messages::Share;

use crate::helpers::ImmutableError;

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct BindMount {
    pub mount_source: PathBuf,
    pub mount_point: PathBuf,
}

impl BindMount {
    pub fn from_share(share: &Share, bind_mount_prefix: &Path) -> Self {
        let mount_point = bind_mount_prefix.join("shares").join(&share.share_id);
        BindMount {
            mount_source: share.get_dest_path(),
            mount_point,
        }
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[error("mount reconciliation failed (mounts: {failed_mounts:?}, umounts: {failed_umounts:?})")]
pub struct ReconciliationError {
    pub failed_mounts: Vec<BindMount>,
    pub failed_umounts: Vec<PathBuf>,
}

#[derive(thiserror::Error, Debug)]
pub enum MountError {
    #[error("pre-mount checks failed: {0}")]
    Preflight(String),
    #[error("mounting failed: {0}")]
    Error(String),
}

impl From<ImmutableError> for MountError {
    fn from(value: ImmutableError) -> Self {
        MountError::Preflight(format!("immutable error: {}", value))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum MountBackendError {
    #[error("failed to list mounts: {0}")]
    List(Box<dyn std::error::Error + Send + Sync>),

    #[error("preflight error: {0}")]
    Preflight(String),

    #[error("create bind mount: {0}")]
    Mount(Box<dyn std::error::Error + Send + Sync>),

    #[error("umount bind mount: {0}")]
    Umount(Box<dyn std::error::Error + Send + Sync>),
}

pub trait MountBackend: std::fmt::Debug + Sync + Send {
    /// create a single bind mount
    fn mount(&self, mount: &BindMount) -> Result<(), MountBackendError>;

    /// pre-flight checks to increase our confidence in a configuration change
    fn preflight(&self, mount: &BindMount) -> Result<(), MountBackendError>;

    /// unmount the currently visible bind mount layer at the given mount point
    fn umount(&self, mount_point: &Path) -> Result<(), MountBackendError>;

    /// list existing bind mount targets in the given prefix
    fn list_mount_points(&self, path_prefix: &Path) -> Result<HashSet<PathBuf>, MountBackendError>;

    /// list targets whose currently visible bind mount layer is stale
    ///
    /// Each mount table record must produce one entry. Stacked mounts therefore
    /// repeat their target path so callers can bound how many layers may need
    /// to be unmounted.
    fn list_stale_mount_points(
        &self,
        path_prefix: &Path,
    ) -> Result<Vec<PathBuf>, MountBackendError>;
}
