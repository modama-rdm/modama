use std::{
    collections::HashSet,
    fs::create_dir_all,
    path::{Path, PathBuf}, sync::Arc,
};

use anyhow::Result;
use nix::mount::MsFlags;
use procfs::process::Process;

use crate::{
    helpers::{have_trailing_slash, keep_immutable, strip_trailing_slash},
    mount::{BindMount, CompoundMountError, MountBackend, MountBackendError},
};

#[derive(Clone, Debug)]
pub struct LinuxMountBackend {}

impl LinuxMountBackend {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for LinuxMountBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MountBackend for LinuxMountBackend {
    fn mount(&self, mount: &BindMount) -> std::result::Result<(), MountBackendError> {
        self.preflight(mount)?;
        // 0) Pre-flight checks: make sure the source is a directory, and it
        //    stays this way. We do this here by setting the immutable flag, and
        //    keeping it until after the mount finished successfully:
        // if we do `chattr +i /some/symlink/` (with a trailing slash),  we
        // actually _follow_ the symlink, and set the link target immutable.
        // Can't have that...
        let src_path = strip_trailing_slash(&mount.mount_source);

        let _guard =
            keep_immutable(&src_path).map_err(|e| MountBackendError::Preflight(e.to_string()))?;
        if src_path.is_symlink() || !src_path.is_dir() {
            Err(MountBackendError::Preflight(format!(
                "source path `{src_path:?}` is not a directory"
            )))?;
        }
        if !mount.mount_point.exists() {
            if let Err(e) = create_dir_all(&mount.mount_point) {
                Err(MountBackendError::Preflight(format!(
                    "could not create mount point `{:?}`: {e}",
                    &mount.mount_point
                )))?;
            }
        }

        nix::mount::mount::<Path, Path, str, str>(
            Some(&src_path),
            &mount.mount_point,
            None,
            MsFlags::MS_BIND,
            None,
        )
        .map_err(|e| MountBackendError::Mount(Box::new(e)))
    }

    /// Pre-flight check that doesn't set the immutable flag. Some code is
    /// duplicated in `mount_single`, where we make sure the given source path
    /// is not modified concurrently.
    fn preflight(&self, mount: &BindMount) -> Result<(), MountBackendError> {
        let src_path = strip_trailing_slash(&mount.mount_source);
        // if we do `chattr +i /some/symlink/` at some point (with a trailing
        // slash),  we actually _follow_ the symlink, and set the link target
        // immutable.  Can't have that... (paranoid, as we remove the trailing
        // slash above)
        if have_trailing_slash(&src_path) {
            Err(MountBackendError::Preflight(format!(
                "source path `{src_path:?}` has trailing slash"
            )))?;
        }
        // make sure the source path is a directory
        if src_path.is_symlink() || !src_path.is_dir() {
            Err(MountBackendError::Preflight(format!(
                "source path `{src_path:?}` is not a directory"
            )))?;
        }

        Ok(())
    }

    fn umount(&self, mount_point: &Path) -> std::result::Result<(), MountBackendError> {
        nix::mount::umount(mount_point).map_err(|e| MountBackendError::Umount(Box::new(e)))
    }

    fn list_mounts(&self, path_prefix: &Path) -> Result<HashSet<BindMount>, MountBackendError> {
        let current_mounts = Process::myself()
            .map_err(|e| MountBackendError::List(Box::new(e)))?
            .mountinfo()
            .map_err(|e| MountBackendError::List(Box::new(e)))?;
        let active_mounts: HashSet<BindMount> = current_mounts
            .into_iter()
            .filter_map(|mi| {
                if let Some(ms) = mi.mount_source {
                    let ms_path = PathBuf::from(ms);
                    if ms_path.starts_with(path_prefix) {
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
        Ok(active_mounts)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SetStateError {
    #[error("mount backend error: {0}")]
    BackendError(#[from] MountBackendError),

    #[error("setting state failed: {0}")]
    MountError(#[from] CompoundMountError),
}

#[derive(Debug)]
pub struct MountManager {
    path_prefix: PathBuf,
    backend: Arc<dyn MountBackend>,
}

impl MountManager {
    pub fn new<B: MountBackend + 'static>(path_prefix: &Path, backend: Arc<B>) -> Self {
        Self {
            path_prefix: path_prefix.to_owned(),
            backend,
        }
    }

    pub fn get_prefix(&self) -> &Path {
        &self.path_prefix
    }

    /// Reconcile state such that the given `desired_mounts` are mounted, and other
    /// mounts in the prefix are umounted.
    pub fn set_state(&self, desired_mounts: HashSet<BindMount>) -> Result<(), SetStateError> {
        let active_mounts = self.backend.list_mounts(&self.path_prefix)?;
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

    pub fn preflight(&self, mount: &BindMount) -> Result<(), SetStateError> {
        Ok(self.backend.preflight(mount)?)
    }

    fn umount<'a>(
        &self,
        bind_mounts: impl Iterator<Item = &'a BindMount>,
    ) -> Result<(), CompoundMountError> {
        // TODO: can probably share some code with `mount` below...
        let failed_mounts: Vec<BindMount> = bind_mounts
            .filter(|bm| {
                if let Err(e) = self.backend.umount(&bm.mount_point) {
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
            Err(CompoundMountError::MultiFailure(failed_mounts))
        }
    }

    fn mount<'a>(
        &self,
        bind_mounts: impl Iterator<Item = &'a BindMount>,
    ) -> Result<(), CompoundMountError> {
        let failed_mounts: Vec<BindMount> = bind_mounts
            .filter(|bm| {
                if let Err(e) = self.backend.mount(bm) {
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
            Err(CompoundMountError::MultiFailure(failed_mounts))
        }
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test {
    use std::{
        collections::HashSet,
        path::{Path, PathBuf},
        str::FromStr, sync::Arc,
    };

    use crate::{
        mount::{BindMount, CompoundMountError, MountBackend, MountBackendError},
        mount_manager::{MountManager, SetStateError},
    };

    #[derive(Clone, Debug)]
    struct MaybeFailMountBackend {}

    #[derive(thiserror::Error, Debug)]
    enum FakeError {
        #[error("stuff")]
        Stuff,
    }

    // fails for mounts with mount_source == /fail/, in mount/umount
    impl MountBackend for MaybeFailMountBackend {
        fn mount(&self, mount: &BindMount) -> Result<(), MountBackendError> {
            self.preflight(mount)?;
            if mount.mount_source.to_str().unwrap() == "/fail/" {
                Err(MountBackendError::Mount(Box::new(FakeError::Stuff)))
            } else {
                Ok(())
            }
        }

        fn umount(&self, mount_point: &Path) -> Result<(), MountBackendError> {
            if mount_point.to_str().unwrap() == "/fail/" {
                Err(MountBackendError::Mount(Box::new(FakeError::Stuff)))
            } else {
                Ok(())
            }
        }

        fn list_mounts(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<BindMount>, MountBackendError> {
            let mut existing_mounts = HashSet::new();
            existing_mounts.insert(BindMount {
                mount_source: PathBuf::from_str("/sauce/1/").unwrap(),
                mount_point: PathBuf::from_str("/fail/").unwrap(),
            });
            existing_mounts.insert(BindMount {
                mount_source: PathBuf::from_str("/sauce/2/").unwrap(),
                mount_point: PathBuf::from_str("/success/").unwrap(),
            });
            Ok(existing_mounts)
        }

        fn preflight(&self, _mount: &BindMount) -> Result<(), MountBackendError> {
            Ok(())
        }
    }

    #[test]
    fn test_happy_case() {
        let backend = Arc::new(MaybeFailMountBackend {});
        let mm = MountManager::new(&PathBuf::from_str("/run/modama").unwrap(), backend);
        assert_eq!(mm.get_prefix(), &PathBuf::from_str("/run/modama").unwrap());

        let mut desired_mounts = HashSet::new();
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/sauce/").unwrap(),
            mount_point: PathBuf::from_str("/run/modama/a").unwrap(),
        });
        // keep the pre-existing mounts:
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/sauce/1/").unwrap(),
            mount_point: PathBuf::from_str("/fail/").unwrap(),
        });
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/sauce/2/").unwrap(),
            mount_point: PathBuf::from_str("/success/").unwrap(),
        });

        mm.set_state(desired_mounts).unwrap();
    }

    #[test]
    fn test_noop_case() {
        let backend = Arc::new(MaybeFailMountBackend {});
        let mm = MountManager::new(&PathBuf::from_str("/run/modama").unwrap(), backend);
        assert_eq!(mm.get_prefix(), &PathBuf::from_str("/run/modama").unwrap());

        let mut desired_mounts = HashSet::new();
        // only the pre-existing mount:
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/sauce/1/").unwrap(),
            mount_point: PathBuf::from_str("/fail/").unwrap(),
        });
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/sauce/2/").unwrap(),
            mount_point: PathBuf::from_str("/success/").unwrap(),
        });
        mm.set_state(desired_mounts).unwrap();
    }

    #[test]
    fn test_umount_success() {
        let backend = Arc::new(MaybeFailMountBackend {});
        let mm = MountManager::new(&PathBuf::from_str("/run/modama").unwrap(), backend);
        assert_eq!(mm.get_prefix(), &PathBuf::from_str("/run/modama").unwrap());

        let mut desired_mounts = HashSet::new();
        // only keep the fail mount:
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/sauce/1/").unwrap(),
            mount_point: PathBuf::from_str("/fail/").unwrap(),
        });
        mm.set_state(desired_mounts).unwrap();
    }


    #[test]
    fn test_mount_failed() {
        let backend = Arc::new(MaybeFailMountBackend {});
        let mm = MountManager::new(&PathBuf::from_str("/run/modama").unwrap(), backend);
        assert_eq!(mm.get_prefix(), &PathBuf::from_str("/run/modama").unwrap());

        let mut desired_mounts = HashSet::new();
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/fail/").unwrap(),
            mount_point: PathBuf::from_str("/run/modama/a").unwrap(),
        });
        // keep the existing mount:
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/sauce/1/").unwrap(),
            mount_point: PathBuf::from_str("/fail/").unwrap(),
        });

        assert!(mm.set_state(desired_mounts).is_err());
    }

    #[test]
    fn test_umount_failed() {
        let backend = Arc::new(MaybeFailMountBackend {});
        let mm = MountManager::new(&PathBuf::from_str("/run/modama").unwrap(), backend);
        assert_eq!(mm.get_prefix(), &PathBuf::from_str("/run/modama").unwrap());

        let mut desired_mounts = HashSet::new();
        desired_mounts.insert(BindMount {
            mount_source: PathBuf::from_str("/sauce/").unwrap(),
            mount_point: PathBuf::from_str("/fail/").unwrap(),
        });

        assert!(mm.set_state(HashSet::new()).is_err());
    }

    #[derive(Clone, Debug)]
    struct PreflightFailMountBackend {}

    impl MountBackend for PreflightFailMountBackend {
        fn mount(&self, mount: &BindMount) -> Result<(), MountBackendError> {
            self.preflight(mount)?;
            Ok(())
        }

        fn umount(&self, _mount_point: &Path) -> Result<(), MountBackendError> {
            Ok(())
        }

        fn list_mounts(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<BindMount>, MountBackendError> {
            Ok(HashSet::new())
        }

        fn preflight(&self, mount: &BindMount) -> Result<(), MountBackendError> {
            if mount.mount_source.to_str().unwrap() == "/fail/" {
                Err(MountBackendError::Preflight(
                    "preconditions not met".to_owned(),
                ))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn test_preflight_failure() {
        let backend = Arc::new(PreflightFailMountBackend {});
        let mm = MountManager::new(&PathBuf::from_str("/run/modama").unwrap(), backend);
        assert_eq!(mm.get_prefix(), &PathBuf::from_str("/run/modama").unwrap());

        let mut desired_mounts = HashSet::new();
        let bm_fail = BindMount {
            mount_source: PathBuf::from_str("/fail/").unwrap(),
            mount_point: PathBuf::from_str("/run/modama/a").unwrap(),
        };
        desired_mounts.insert(bm_fail.clone());
        let bm_succ = BindMount {
            mount_source: PathBuf::from_str("/succeed/").unwrap(),
            mount_point: PathBuf::from_str("/run/modama/b").unwrap(),
        };
        desired_mounts.insert(bm_succ.clone());

        // explicit preflight check:
        assert!(mm.preflight(&bm_fail).is_err());

        // `mount` should also do a preflight check; although it depends on the implementation.
        // here we test that the requested mount is in the list of failed mounts:
        // via set_state -> mount -> preflight:
        let res = mm.set_state(desired_mounts);
        match res {
            Err(SetStateError::MountError(CompoundMountError::MultiFailure(failed_mounts))) => {
                // bm_succ is not in here:
                assert_eq!(vec![bm_fail], failed_mounts);
            }
            x => panic!("should not happen: {x:?}"),
        }
    }
}
