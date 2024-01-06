use std::{
    collections::HashSet,
    fs::create_dir,
    path::{Path, PathBuf},
};

use anyhow::Result;
use nix::mount::MsFlags;
use procfs::{process::Process, ProcError};

use crate::helpers::{
    have_trailing_slash, keep_immutable, strip_trailing_slash, IOCFlagsError, ImmutableError,
};

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct BindMount {
    pub mount_source: PathBuf,
    pub mount_point: PathBuf,
}

pub struct MountManager {
    path_prefix: PathBuf,
}

#[derive(thiserror::Error, Debug)]
pub enum CompoundMountError {
    #[error("one or more mounts failed")]
    Error(Vec<BindMount>),
}

#[derive(thiserror::Error, Debug)]
pub enum MountError {
    #[error("pre-mount checks failed")]
    Preflight(String),
    #[error("mounting failed")]
    Error(String),
}

#[derive(thiserror::Error, Debug)]
pub enum SetStateError {
    #[error("error getting current mounts")]
    ProcError(#[from] ProcError),

    #[error("setting state failed")]
    MountError(#[from] CompoundMountError),
}

impl From<IOCFlagsError> for MountError {
    fn from(value: IOCFlagsError) -> Self {
        MountError::Preflight(value.to_string())
    }
}

impl From<ImmutableError> for MountError {
    fn from(value: ImmutableError) -> Self {
        MountError::Preflight(value.to_string())
    }
}

impl MountManager {
    pub fn new(path_prefix: &Path) -> Self {
        Self {
            path_prefix: path_prefix.to_owned(),
        }
    }

    pub fn get_prefix(&self) -> &Path {
        &self.path_prefix
    }

    /// Reconcile state such that the given `desired_mounts` are mounted, and other
    /// mounts in the prefix are umounted.
    pub fn set_state(&self, desired_mounts: HashSet<BindMount>) -> Result<(), SetStateError> {
        let current_mounts = Process::myself()?.mountinfo()?;

        let active_mounts: HashSet<BindMount> = current_mounts
            .into_iter()
            .filter_map(|mi| {
                if let Some(ms) = mi.mount_source {
                    let ms_path = PathBuf::from(ms);
                    if ms_path.starts_with(&self.path_prefix) {
                        Some(BindMount {
                            mount_source: ms_path.canonicalize().unwrap_or(ms_path),
                            mount_point: mi.mount_point.canonicalize().unwrap_or(mi.mount_point),
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        let to_umount = active_mounts.difference(&desired_mounts);
        let to_mount = desired_mounts.difference(&active_mounts);

        // try both mounts and umounts, but prioritize errors from mounting, as we must not
        // create shares for destinations where the bind mount failed, but we may remove a share
        // while a mount still exists.
        let mount_res = self.mount(to_mount);
        let umount_res = self.umount(to_umount);
        mount_res?;
        umount_res?;

        Ok(())
    }

    fn umount<'a>(
        &self,
        bind_mounts: impl Iterator<Item = &'a BindMount>,
    ) -> Result<(), CompoundMountError> {
        // TODO: can probably share some code with `mount` below...
        let failed_mounts: Vec<BindMount> = bind_mounts
            .filter(|bm| {
                if let Err(e) = nix::mount::umount(&bm.mount_point) {
                    log::error!("umount of {:?} failed: {e}", &bm.mount_point);
                    true
                } else {
                    false
                }
            })
            .cloned()
            .collect();
        if failed_mounts.is_empty() {
            Ok(())
        } else {
            Err(CompoundMountError::Error(failed_mounts))
        }
    }

    fn mount_single(&self, mount: &BindMount) -> Result<(), MountError> {
        // 0) Pre-flight checks: make sure the source is a directory, and it
        //    stays this way. We do this here by setting the immutable flag, and
        //    keeping it until after the mount finished successfully:
        let src_path = strip_trailing_slash(&mount.mount_source);
        // if we do `chattr +i /some/symlink/` (with a trailing slash),  we
        // actually _follow_ the symlink, and set the link target immutable.
        // Can't have that...
        if have_trailing_slash(&src_path) {
            Err(MountError::Preflight(format!(
                "source path `{src_path:?}` has trailing slash"
            )))?;
        }
        let _guard = keep_immutable(&src_path)?;
        if src_path.is_symlink() || !src_path.is_dir() {
            Err(MountError::Preflight(format!(
                "source path `{src_path:?}` is not a directory"
            )))?;
        }
        if !mount.mount_point.exists() {
            if let Err(e) = create_dir(&mount.mount_point) {
                Err(MountError::Preflight(format!(
                    "could not create mount point `{:?}`: {e}",
                    &mount.mount_point
                )))?;
            }
        }
        if let Err(e) = nix::mount::mount::<PathBuf, Path, str, str>(
            Some(&src_path),
            &mount.mount_point,
            None,
            MsFlags::empty(),
            Some("bind"),
        ) {
            Err(MountError::Error(e.to_string()))
        } else {
            Ok(())
        }
    }

    fn mount<'a>(
        &self,
        bind_mounts: impl Iterator<Item = &'a BindMount>,
    ) -> Result<(), CompoundMountError> {
        let failed_mounts: Vec<BindMount> = bind_mounts
            .filter(|bm| {
                if let Err(e) = self.mount_single(bm) {
                    log::error!(
                        "bind mount from {:?} to {:?} failed: {e}",
                        &bm.mount_source,
                        &bm.mount_point
                    );
                    true
                } else {
                    false
                }
            })
            .cloned()
            .collect();
        if failed_mounts.is_empty() {
            Ok(())
        } else {
            Err(CompoundMountError::Error(failed_mounts))
        }
    }
}
