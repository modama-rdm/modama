use std::{
    collections::HashSet,
    fs::create_dir_all,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use nix::mount::MsFlags;
use procfs::process::{MountInfo, Process};

use crate::{
    helpers::{have_trailing_slash, keep_immutable, strip_trailing_slash},
    mount::{BindMount, CompoundMountError, MountBackend, MountBackendError},
};

#[derive(Clone, Debug)]
pub struct LinuxMountBackend {}

fn is_managed_mount(mount: &MountInfo, path_prefix: &Path) -> bool {
    mount.mount_point.starts_with(path_prefix.join("shares"))
}

impl LinuxMountBackend {
    pub fn new() -> Self {
        Self {}
    }

    fn is_stale_mount_point(&self, mount_point: &Path) -> Result<bool, MountBackendError> {
        let is_mount_point = Process::myself()
            .map_err(|e| MountBackendError::List(Box::new(e)))?
            .mountinfo()
            .map_err(|e| MountBackendError::List(Box::new(e)))?
            .into_iter()
            .any(|mount| mount.mount_point == mount_point);

        if !is_mount_point {
            return Ok(false);
        }

        match std::fs::metadata(mount_point) {
            Ok(metadata) => Ok(metadata.nlink() == 0),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(MountBackendError::List(Box::new(error))),
        }
    }
}

impl Default for LinuxMountBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MountBackend for LinuxMountBackend {
    fn mount(&self, mount: &BindMount) -> std::result::Result<(), MountBackendError> {
        // FIXME: Once MountManager::set_state reconciles by mountpoint, make this
        // reject every already-mounted target instead of only stale targets.
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
                    mount.mount_point
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

        // FIXME: Once MountManager::set_state reconciles by mountpoint, reject
        // every already-mounted target here instead of only stale targets.
        if self.is_stale_mount_point(&mount.mount_point)? {
            return Err(MountBackendError::Preflight(format!(
                "mount point `{:?}` contains a stale mount",
                mount.mount_point
            )));
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

    fn list_stale_mount_points(
        &self,
        path_prefix: &Path,
    ) -> Result<HashSet<PathBuf>, MountBackendError> {
        let current_mounts = Process::myself()
            .map_err(|e| MountBackendError::List(Box::new(e)))?
            .mountinfo()
            .map_err(|e| MountBackendError::List(Box::new(e)))?;

        current_mounts
            .into_iter()
            .filter(|mount| is_managed_mount(mount, path_prefix))
            .filter_map(|mount| match std::fs::metadata(&mount.mount_point) {
                Ok(metadata) if metadata.nlink() == 0 => Some(Ok(mount.mount_point)),
                Ok(_) => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => Some(Err(MountBackendError::List(Box::new(error)))),
            })
            .collect()
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
        self.cleanup_stale_mounts()?;

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

    fn cleanup_stale_mounts(&self) -> Result<(), SetStateError> {
        let stale_mount_points = self.backend.list_stale_mount_points(&self.path_prefix)?;
        for mount_point in &stale_mount_points {
            log::info!("unmounting stale bind mount at {mount_point:?}");
        }

        let failed_mount_points =
            self.umount_mount_points(stale_mount_points.iter().map(std::path::PathBuf::as_path));
        if !failed_mount_points.is_empty() {
            log::warn!(
                "continuing reconciliation after stale mount cleanup failed for {failed_mount_points:?}"
            );
        }
        Ok(())
    }

    fn umount_mount_points<'a>(
        &self,
        mount_points: impl Iterator<Item = &'a Path>,
    ) -> Vec<PathBuf> {
        mount_points
            .filter_map(|mount_point| {
                if let Err(error) = self.backend.umount(mount_point) {
                    log::error!("umount of {mount_point:?} failed: {error}");
                    Some(mount_point.to_owned())
                } else {
                    None
                }
            })
            .collect()
    }

    fn umount<'a>(
        &self,
        bind_mounts: impl Iterator<Item = &'a BindMount>,
    ) -> Result<(), CompoundMountError> {
        let bind_mounts: Vec<&BindMount> = bind_mounts.collect();
        let failed_mount_points: HashSet<PathBuf> = self
            .umount_mount_points(bind_mounts.iter().map(|mount| mount.mount_point.as_path()))
            .into_iter()
            .collect();
        let failed_mounts: Vec<BindMount> = bind_mounts
            .into_iter()
            .filter(|mount| failed_mount_points.contains(&mount.mount_point))
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
                        bm.mount_source,
                        bm.mount_point
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
        str::FromStr,
        sync::{Arc, Mutex},
    };

    use procfs::process::MountInfo;

    use crate::{
        mount::{BindMount, CompoundMountError, MountBackend, MountBackendError},
        mount_manager::{MountManager, SetStateError},
    };

    use super::is_managed_mount;

    #[derive(Clone, Debug)]
    struct MaybeFailMountBackend {}

    #[derive(thiserror::Error, Debug)]
    enum FakeError {
        #[error("stuff")]
        Stuff,
    }

    #[derive(Clone, Debug)]
    struct StaleCleanupBackend {
        calls: Arc<Mutex<Vec<&'static str>>>,
        stale_mount_point: PathBuf,
        fail_umount: bool,
    }

    impl StaleCleanupBackend {
        fn new(stale_mount_point: PathBuf, fail_umount: bool) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                stale_mount_point,
                fail_umount,
            }
        }
    }

    impl MountBackend for StaleCleanupBackend {
        fn mount(&self, mount: &BindMount) -> Result<(), MountBackendError> {
            self.calls.lock().unwrap().push("mount");
            self.preflight(mount)
        }

        fn preflight(&self, mount: &BindMount) -> Result<(), MountBackendError> {
            if self.fail_umount && mount.mount_point == self.stale_mount_point {
                Err(MountBackendError::Preflight(
                    "mount point contains a stale mount".to_owned(),
                ))
            } else {
                Ok(())
            }
        }

        fn umount(&self, _mount_point: &Path) -> Result<(), MountBackendError> {
            self.calls.lock().unwrap().push("umount");
            if self.fail_umount {
                Err(MountBackendError::Umount(Box::new(FakeError::Stuff)))
            } else {
                Ok(())
            }
        }

        fn list_mounts(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<BindMount>, MountBackendError> {
            self.calls.lock().unwrap().push("list_mounts");
            Ok(HashSet::new())
        }

        fn list_stale_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<PathBuf>, MountBackendError> {
            self.calls.lock().unwrap().push("list_stale_mounts");
            Ok(HashSet::from([self.stale_mount_point.clone()]))
        }
    }

    #[test]
    fn stale_mount_is_cleaned_before_reconciliation() {
        let mount_point = PathBuf::from("/run/modama/shares/stale");
        let backend = Arc::new(StaleCleanupBackend::new(mount_point.clone(), false));
        let mm = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let desired_mount = BindMount {
            mount_source: PathBuf::from("/source"),
            mount_point,
        };

        mm.set_state(HashSet::from([desired_mount])).unwrap();

        assert_eq!(
            *backend.calls.lock().unwrap(),
            ["list_stale_mounts", "umount", "list_mounts", "mount"]
        );
    }

    #[test]
    fn stale_unmount_failure_does_not_stop_reconciliation() {
        let stale_mount_point = PathBuf::from("/run/modama/shares/stale");
        let healthy_mount_point = PathBuf::from("/run/modama/shares/healthy");
        let backend = Arc::new(StaleCleanupBackend::new(stale_mount_point.clone(), true));
        let mm = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let stale_mount = BindMount {
            mount_source: PathBuf::from("/stale-source"),
            mount_point: stale_mount_point,
        };
        let healthy_mount = BindMount {
            mount_source: PathBuf::from("/healthy-source"),
            mount_point: healthy_mount_point,
        };

        let result = mm.set_state(HashSet::from([stale_mount.clone(), healthy_mount]));

        assert!(matches!(
            result,
            Err(SetStateError::MountError(CompoundMountError::MultiFailure(failed)))
                if failed == vec![stale_mount]
        ));
        assert_eq!(
            *backend.calls.lock().unwrap(),
            [
                "list_stale_mounts",
                "umount",
                "list_mounts",
                "mount",
                "mount"
            ]
        );
    }

    #[test]
    fn undesired_stale_unmount_failure_does_not_fail_reconciliation() {
        let stale_mount_point = PathBuf::from("/run/modama/shares/stale");
        let backend = Arc::new(StaleCleanupBackend::new(stale_mount_point, true));
        let mm = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let healthy_mount = BindMount {
            mount_source: PathBuf::from("/healthy-source"),
            mount_point: PathBuf::from("/run/modama/shares/healthy"),
        };

        mm.set_state(HashSet::from([healthy_mount])).unwrap();

        assert_eq!(
            *backend.calls.lock().unwrap(),
            ["list_stale_mounts", "umount", "list_mounts", "mount"]
        );
    }

    #[test]
    fn managed_mount_is_selected_by_mount_point() {
        let mount = MountInfo::from_line(
            "818 573 9:0 /alex/source/data-management-system/modama-rs/test-env/testfolder/data/adhoc/testproject/session2 /run/smbdrop/shares/f923ad36-a557-4d66-b2f4-a0bdc4e49e8a rw,relatime - ext4 /dev/md0 rw,errors=remount-ro,stripe=256",
        )
        .unwrap();

        assert!(is_managed_mount(&mount, Path::new("/run/smbdrop")));
        assert_eq!(mount.mount_source.as_deref(), Some("/dev/md0"));
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

        fn list_stale_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<PathBuf>, MountBackendError> {
            Ok(HashSet::new())
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

        fn list_stale_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<PathBuf>, MountBackendError> {
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
