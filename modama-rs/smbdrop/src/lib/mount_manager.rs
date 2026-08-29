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
    mount::{BindMount, MountBackend, MountBackendError, ReconciliationError},
};

#[derive(Clone, Debug)]
pub struct LinuxMountBackend {}

fn is_managed_mount(mount: &MountInfo, path_prefix: &Path) -> bool {
    let shares_path = path_prefix.join("shares");
    mount.mount_point != shares_path && mount.mount_point.starts_with(shares_path)
}

impl LinuxMountBackend {
    pub fn new() -> Self {
        Self {}
    }

    fn is_mount_point(&self, mount_point: &Path) -> Result<bool, MountBackendError> {
        Ok(Process::myself()
            .map_err(|e| MountBackendError::List(Box::new(e)))?
            .mountinfo()
            .map_err(|e| MountBackendError::List(Box::new(e)))?
            .into_iter()
            .any(|mount| mount.mount_point == mount_point))
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
        if !mount.mount_point.exists()
            && let Err(e) = create_dir_all(&mount.mount_point)
        {
            Err(MountBackendError::Preflight(format!(
                "could not create mount point `{:?}`: {e}",
                mount.mount_point
            )))?;
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

        if self.is_mount_point(&mount.mount_point)? {
            return Err(MountBackendError::Preflight(format!(
                "mount point `{:?}` already contains a mount",
                mount.mount_point
            )));
        }

        Ok(())
    }

    fn umount(&self, mount_point: &Path) -> std::result::Result<(), MountBackendError> {
        nix::mount::umount(mount_point).map_err(|e| MountBackendError::Umount(Box::new(e)))
    }

    fn list_mount_points(&self, path_prefix: &Path) -> Result<HashSet<PathBuf>, MountBackendError> {
        Ok(Process::myself()
            .map_err(|e| MountBackendError::List(Box::new(e)))?
            .mountinfo()
            .map_err(|e| MountBackendError::List(Box::new(e)))?
            .into_iter()
            .filter(|mount| is_managed_mount(mount, path_prefix))
            .map(|mount| mount.mount_point)
            .collect())
    }

    fn list_stale_mount_points(
        &self,
        path_prefix: &Path,
    ) -> Result<Vec<PathBuf>, MountBackendError> {
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
    Reconciliation(#[from] ReconciliationError),
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
        let failed_stale_umounts = self.cleanup_stale_mounts()?;
        let active_mount_points = self.backend.list_mount_points(&self.path_prefix)?;
        let desired_mount_points: HashSet<&Path> = desired_mounts
            .iter()
            .map(|mount| mount.mount_point.as_path())
            .collect();
        let failed_stale_umounts: HashSet<PathBuf> = failed_stale_umounts
            .into_iter()
            .filter(|mount_point| active_mount_points.contains(mount_point))
            .collect();

        let mut failed_mounts: Vec<BindMount> = desired_mounts
            .iter()
            .filter(|mount| failed_stale_umounts.contains(&mount.mount_point))
            .cloned()
            .collect();
        let to_mount = desired_mounts
            .iter()
            .filter(|mount| !active_mount_points.contains(&mount.mount_point));
        let to_umount: Vec<PathBuf> = active_mount_points
            .iter()
            .filter(|mount_point| {
                !desired_mount_points.contains(mount_point.as_path())
                    && !failed_stale_umounts.contains(*mount_point)
            })
            .cloned()
            .collect();

        let mut failed_umounts: Vec<PathBuf> = failed_stale_umounts.into_iter().collect();
        failed_umounts
            .extend(self.umount_mount_points(to_umount.iter().map(std::path::PathBuf::as_path)));
        failed_mounts.extend(self.mount(to_mount));

        failed_mounts.sort_by(|left, right| left.mount_point.cmp(&right.mount_point));
        failed_umounts.sort();
        if failed_mounts.is_empty() && failed_umounts.is_empty() {
            Ok(())
        } else {
            Err(ReconciliationError {
                failed_mounts,
                failed_umounts,
            }
            .into())
        }
    }

    pub fn preflight(&self, mount: &BindMount) -> Result<(), SetStateError> {
        Ok(self.backend.preflight(mount)?)
    }

    fn cleanup_stale_mounts(&self) -> Result<Vec<PathBuf>, SetStateError> {
        // Keep one entry per mount table record here: duplicate paths are the
        // stacked layers that bound the number of cleanup rounds.
        let stale_mount_points = self.backend.list_stale_mount_points(&self.path_prefix)?;
        let stale_mount_layer_limit = stale_mount_points.len();
        let mut stale_mount_points: HashSet<PathBuf> = stale_mount_points.into_iter().collect();
        let mut failed_mount_points = HashSet::new();

        for round in 0..stale_mount_layer_limit {
            let mount_points_to_unmount: Vec<PathBuf> = stale_mount_points
                .difference(&failed_mount_points)
                .cloned()
                .collect();
            if mount_points_to_unmount.is_empty() {
                break;
            }

            for mount_point in &mount_points_to_unmount {
                log::info!("unmounting stale bind mount at {mount_point:?}");
            }

            let failed_in_round = self.umount_mount_points(
                mount_points_to_unmount
                    .iter()
                    .map(std::path::PathBuf::as_path),
            );
            let all_unmounts_failed = failed_in_round.len() == mount_points_to_unmount.len();
            failed_mount_points.extend(failed_in_round);

            // Failed targets are left for the next reconciliation pass. Only
            // rescan when another bounded round can make progress.
            if all_unmounts_failed || round + 1 == stale_mount_layer_limit {
                break;
            }
            stale_mount_points = self
                .backend
                .list_stale_mount_points(&self.path_prefix)?
                .into_iter()
                .collect();
        }

        let mut failed_mount_points: Vec<PathBuf> = failed_mount_points.into_iter().collect();
        failed_mount_points.sort();
        Ok(failed_mount_points)
    }

    fn umount_mount_points<'a>(
        &self,
        mount_points: impl Iterator<Item = &'a Path>,
    ) -> Vec<PathBuf> {
        let mut mount_points: Vec<PathBuf> = mount_points.map(Path::to_owned).collect();
        mount_points.sort_by(|left, right| {
            right
                .components()
                .count()
                .cmp(&left.components().count())
                .then_with(|| left.cmp(right))
        });
        mount_points
            .into_iter()
            .filter_map(|mount_point| {
                if let Err(error) = self.backend.umount(&mount_point) {
                    log::error!("umount of {mount_point:?} failed: {error}");
                    Some(mount_point)
                } else {
                    None
                }
            })
            .collect()
    }

    fn mount<'a>(&self, bind_mounts: impl Iterator<Item = &'a BindMount>) -> Vec<BindMount> {
        let mut bind_mounts: Vec<&BindMount> = bind_mounts.collect();
        bind_mounts.sort_by(|left, right| left.mount_point.cmp(&right.mount_point));
        bind_mounts
            .into_iter()
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
            .collect()
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test {
    use std::{
        collections::{HashMap, HashSet},
        path::{Path, PathBuf},
        str::FromStr,
        sync::{Arc, Mutex},
    };

    use procfs::process::MountInfo;

    use crate::{
        mount::{BindMount, MountBackend, MountBackendError, ReconciliationError},
        mount_manager::{LinuxMountBackend, MountManager, SetStateError},
    };

    use super::is_managed_mount;

    #[derive(Clone, Debug)]
    struct MaybeFailMountBackend {}

    #[derive(thiserror::Error, Debug)]
    enum FakeError {
        #[error("stuff")]
        Stuff,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum ReconciliationCall {
        Mount(PathBuf),
        Umount(PathBuf),
    }

    #[derive(Clone, Debug)]
    struct ReconciliationBackend {
        active_mount_points: HashSet<PathBuf>,
        calls: Arc<Mutex<Vec<ReconciliationCall>>>,
        failed_mount_source: Option<PathBuf>,
        failed_umount: Option<PathBuf>,
    }

    impl MountBackend for ReconciliationBackend {
        fn mount(&self, mount: &BindMount) -> Result<(), MountBackendError> {
            self.calls
                .lock()
                .unwrap()
                .push(ReconciliationCall::Mount(mount.mount_point.clone()));
            if self.failed_mount_source.as_ref() == Some(&mount.mount_source) {
                Err(MountBackendError::Mount(Box::new(FakeError::Stuff)))
            } else {
                Ok(())
            }
        }

        fn preflight(&self, _mount: &BindMount) -> Result<(), MountBackendError> {
            Ok(())
        }

        fn umount(&self, mount_point: &Path) -> Result<(), MountBackendError> {
            self.calls
                .lock()
                .unwrap()
                .push(ReconciliationCall::Umount(mount_point.to_owned()));
            if self.failed_umount.as_deref() == Some(mount_point) {
                Err(MountBackendError::Umount(Box::new(FakeError::Stuff)))
            } else {
                Ok(())
            }
        }

        fn list_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<PathBuf>, MountBackendError> {
            Ok(self.active_mount_points.clone())
        }

        fn list_stale_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<Vec<PathBuf>, MountBackendError> {
            Ok(Vec::new())
        }
    }

    #[derive(Clone, Debug)]
    struct StaleCleanupBackend {
        calls: Arc<Mutex<Vec<StaleCleanupCall>>>,
        stacks: Arc<Mutex<HashMap<PathBuf, StaleStack>>>,
        failed_umounts: HashSet<PathBuf>,
    }

    impl StaleCleanupBackend {
        fn new(
            stacks: impl IntoIterator<Item = (PathBuf, StaleStack)>,
            failed_umounts: HashSet<PathBuf>,
        ) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                stacks: Arc::new(Mutex::new(stacks.into_iter().collect())),
                failed_umounts,
            }
        }

        fn single(
            mount_point: PathBuf,
            stale_layers: usize,
            live_layer: bool,
            fail_umount: bool,
        ) -> Self {
            let failed_umounts = if fail_umount {
                HashSet::from([mount_point.clone()])
            } else {
                HashSet::new()
            };
            Self::new(
                [(
                    mount_point,
                    StaleStack {
                        stale_layers,
                        live_layer,
                    },
                )],
                failed_umounts,
            )
        }
    }

    #[derive(Clone, Debug)]
    struct StaleStack {
        stale_layers: usize,
        live_layer: bool,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum StaleCleanupCall {
        ListStaleMounts,
        ListMountPoints,
        Mount(PathBuf),
        Umount(PathBuf),
    }

    impl MountBackend for StaleCleanupBackend {
        fn mount(&self, mount: &BindMount) -> Result<(), MountBackendError> {
            self.calls
                .lock()
                .unwrap()
                .push(StaleCleanupCall::Mount(mount.mount_point.clone()));
            self.preflight(mount)
        }

        fn preflight(&self, mount: &BindMount) -> Result<(), MountBackendError> {
            let is_active = self
                .stacks
                .lock()
                .unwrap()
                .get(&mount.mount_point)
                .is_some_and(|stack| stack.stale_layers > 0 || stack.live_layer);
            if is_active {
                Err(MountBackendError::Preflight(
                    "mount point already contains a mount".to_owned(),
                ))
            } else {
                Ok(())
            }
        }

        fn umount(&self, mount_point: &Path) -> Result<(), MountBackendError> {
            self.calls
                .lock()
                .unwrap()
                .push(StaleCleanupCall::Umount(mount_point.to_owned()));
            if self.failed_umounts.contains(mount_point) {
                return Err(MountBackendError::Umount(Box::new(FakeError::Stuff)));
            }

            let mut stacks = self.stacks.lock().unwrap();
            let stack = stacks.get_mut(mount_point).unwrap();
            assert!(stack.stale_layers > 0);
            stack.stale_layers -= 1;
            Ok(())
        }

        fn list_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<PathBuf>, MountBackendError> {
            self.calls
                .lock()
                .unwrap()
                .push(StaleCleanupCall::ListMountPoints);
            Ok(self
                .stacks
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, stack)| stack.stale_layers > 0 || stack.live_layer)
                .map(|(mount_point, _)| mount_point.clone())
                .collect())
        }

        fn list_stale_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<Vec<PathBuf>, MountBackendError> {
            self.calls
                .lock()
                .unwrap()
                .push(StaleCleanupCall::ListStaleMounts);
            Ok(self
                .stacks
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, stack)| stack.stale_layers > 0)
                .flat_map(|(mount_point, stack)| {
                    std::iter::repeat_n(
                        mount_point.clone(),
                        stack.stale_layers + usize::from(stack.live_layer),
                    )
                })
                .collect())
        }
    }

    #[test]
    fn stale_mount_is_cleaned_before_reconciliation() {
        let mount_point = PathBuf::from("/run/modama/shares/stale");
        let backend = Arc::new(StaleCleanupBackend::single(
            mount_point.clone(),
            1,
            false,
            false,
        ));
        let mm = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let desired_mount = BindMount {
            mount_source: PathBuf::from("/source"),
            mount_point: mount_point.clone(),
        };

        mm.set_state(HashSet::from([desired_mount])).unwrap();

        assert_eq!(
            *backend.calls.lock().unwrap(),
            [
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(mount_point.clone()),
                StaleCleanupCall::ListMountPoints,
                StaleCleanupCall::Mount(mount_point),
            ]
        );
    }

    #[test]
    fn stacked_stale_mounts_are_cleaned_before_reconciliation() {
        let mount_point = PathBuf::from("/run/modama/shares/stale");
        let backend = Arc::new(StaleCleanupBackend::single(
            mount_point.clone(),
            3,
            false,
            false,
        ));
        let manager = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let desired_mount = BindMount {
            mount_source: PathBuf::from("/source"),
            mount_point: mount_point.clone(),
        };

        manager.set_state(HashSet::from([desired_mount])).unwrap();

        assert_eq!(
            *backend.calls.lock().unwrap(),
            [
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(mount_point.clone()),
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(mount_point.clone()),
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(mount_point.clone()),
                StaleCleanupCall::ListMountPoints,
                StaleCleanupCall::Mount(mount_point),
            ]
        );
    }

    #[test]
    fn stale_cleanup_stops_after_uncovering_a_live_layer() {
        let mount_point = PathBuf::from("/run/modama/shares/stale");
        let backend = Arc::new(StaleCleanupBackend::single(
            mount_point.clone(),
            2,
            true,
            false,
        ));
        let manager = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let desired_mount = BindMount {
            mount_source: PathBuf::from("/source"),
            mount_point: mount_point.clone(),
        };

        manager.set_state(HashSet::from([desired_mount])).unwrap();

        assert_eq!(
            *backend.calls.lock().unwrap(),
            [
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(mount_point.clone()),
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(mount_point),
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::ListMountPoints,
            ]
        );
    }

    #[test]
    fn stale_unmount_failure_does_not_stop_reconciliation() {
        let stale_mount_point = PathBuf::from("/run/modama/shares/stale");
        let healthy_mount_point = PathBuf::from("/run/modama/shares/healthy");
        let backend = Arc::new(StaleCleanupBackend::single(
            stale_mount_point.clone(),
            1,
            false,
            true,
        ));
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
            Err(SetStateError::Reconciliation(ReconciliationError {
                failed_mounts,
                failed_umounts,
            })) if failed_mounts == vec![stale_mount]
                && failed_umounts == vec![PathBuf::from("/run/modama/shares/stale")]
        ));
        assert_eq!(
            *backend.calls.lock().unwrap(),
            [
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(PathBuf::from("/run/modama/shares/stale")),
                StaleCleanupCall::ListMountPoints,
                StaleCleanupCall::Mount(PathBuf::from("/run/modama/shares/healthy")),
            ]
        );
    }

    #[test]
    fn undesired_stale_unmount_failure_does_not_fail_reconciliation() {
        let stale_mount_point = PathBuf::from("/run/modama/shares/stale");
        let backend = Arc::new(StaleCleanupBackend::single(
            stale_mount_point,
            1,
            false,
            true,
        ));
        let mm = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let healthy_mount = BindMount {
            mount_source: PathBuf::from("/healthy-source"),
            mount_point: PathBuf::from("/run/modama/shares/healthy"),
        };

        let result = mm.set_state(HashSet::from([healthy_mount]));

        assert!(matches!(
            result,
            Err(SetStateError::Reconciliation(ReconciliationError {
                failed_mounts,
                failed_umounts,
            })) if failed_mounts.is_empty()
                && failed_umounts == vec![PathBuf::from("/run/modama/shares/stale")]
        ));
        assert_eq!(
            *backend.calls.lock().unwrap(),
            [
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(PathBuf::from("/run/modama/shares/stale")),
                StaleCleanupCall::ListMountPoints,
                StaleCleanupCall::Mount(PathBuf::from("/run/modama/shares/healthy")),
            ]
        );
    }

    #[test]
    fn failed_stale_stack_does_not_block_other_cleanup_or_mounts() {
        let failed_mount_point = PathBuf::from("/run/modama/shares/failed");
        let recoverable_mount_point = PathBuf::from("/run/modama/shares/recoverable");
        let healthy_mount_point = PathBuf::from("/run/modama/shares/healthy");
        let backend = Arc::new(StaleCleanupBackend::new(
            [
                (
                    failed_mount_point.clone(),
                    StaleStack {
                        stale_layers: 2,
                        live_layer: false,
                    },
                ),
                (
                    recoverable_mount_point.clone(),
                    StaleStack {
                        stale_layers: 2,
                        live_layer: false,
                    },
                ),
            ],
            HashSet::from([failed_mount_point.clone()]),
        ));
        let manager = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let failed_mount = BindMount {
            mount_source: PathBuf::from("/failed-source"),
            mount_point: failed_mount_point.clone(),
        };
        let recoverable_mount = BindMount {
            mount_source: PathBuf::from("/recoverable-source"),
            mount_point: recoverable_mount_point.clone(),
        };
        let healthy_mount = BindMount {
            mount_source: PathBuf::from("/healthy-source"),
            mount_point: healthy_mount_point.clone(),
        };

        let result = manager.set_state(HashSet::from([
            failed_mount.clone(),
            recoverable_mount,
            healthy_mount,
        ]));

        assert!(matches!(
            result,
            Err(SetStateError::Reconciliation(ReconciliationError {
                failed_mounts,
                failed_umounts,
            })) if failed_mounts == vec![failed_mount]
                && failed_umounts == vec![failed_mount_point.clone()]
        ));
        assert_eq!(
            *backend.calls.lock().unwrap(),
            [
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(failed_mount_point),
                StaleCleanupCall::Umount(recoverable_mount_point.clone()),
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::Umount(recoverable_mount_point.clone()),
                StaleCleanupCall::ListStaleMounts,
                StaleCleanupCall::ListMountPoints,
                StaleCleanupCall::Mount(healthy_mount_point),
                StaleCleanupCall::Mount(recoverable_mount_point),
            ]
        );
    }

    #[test]
    fn managed_mount_is_selected_by_mount_point() {
        let mut mount = MountInfo::from_line(
            "818 573 9:0 /alex/source/data-management-system/modama-rs/test-env/testfolder/data/adhoc/testproject/session2 /run/smbdrop/shares/f923ad36-a557-4d66-b2f4-a0bdc4e49e8a rw,relatime - ext4 /dev/md0 rw,errors=remount-ro,stripe=256",
        )
        .unwrap();

        assert!(is_managed_mount(&mount, Path::new("/run/smbdrop")));
        assert_eq!(mount.mount_source.as_deref(), Some("/dev/md0"));

        mount.mount_point = PathBuf::from("/run/smbdrop/shares/share-id/nested");
        assert!(is_managed_mount(&mount, Path::new("/run/smbdrop")));

        mount.mount_point = PathBuf::from("/run/smbdrop/shares");
        assert!(!is_managed_mount(&mount, Path::new("/run/smbdrop")));
    }

    #[test]
    fn linux_preflight_rejects_only_an_exact_existing_mount_point() {
        let backend = LinuxMountBackend::new();
        let existing_target = BindMount {
            mount_source: PathBuf::from("/tmp"),
            mount_point: PathBuf::from("/proc"),
        };
        let target_below_existing_mount = BindMount {
            mount_source: PathBuf::from("/tmp"),
            mount_point: PathBuf::from("/proc/smbdrop-nonexistent-mount-target"),
        };

        assert!(matches!(
            backend.preflight(&existing_target),
            Err(MountBackendError::Preflight(message))
                if message.contains("already contains a mount")
        ));
        backend.preflight(&target_below_existing_mount).unwrap();
    }

    #[test]
    fn active_mount_point_is_satisfied_without_comparing_source() {
        let mount_point = PathBuf::from("/run/modama/shares/existing");
        let backend = Arc::new(ReconciliationBackend {
            active_mount_points: HashSet::from([mount_point.clone()]),
            calls: Arc::new(Mutex::new(Vec::new())),
            failed_mount_source: Some(PathBuf::from("/unresolvable/source")),
            failed_umount: None,
        });
        let manager = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));
        let desired_mount = BindMount {
            mount_source: PathBuf::from("/unresolvable/source"),
            mount_point,
        };

        manager.set_state(HashSet::from([desired_mount])).unwrap();

        assert!(backend.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn nested_umounts_and_mounts_are_all_attempted_before_returning_failures() {
        let parent = PathBuf::from("/run/modama/shares/orphan");
        let child = parent.join("nested");
        let failed_mount = BindMount {
            mount_source: PathBuf::from("/fail-mount"),
            mount_point: PathBuf::from("/run/modama/shares/new-a"),
        };
        let successful_mount = BindMount {
            mount_source: PathBuf::from("/success-mount"),
            mount_point: PathBuf::from("/run/modama/shares/new-b"),
        };
        let backend = Arc::new(ReconciliationBackend {
            active_mount_points: HashSet::from([parent.clone(), child.clone()]),
            calls: Arc::new(Mutex::new(Vec::new())),
            failed_mount_source: Some(failed_mount.mount_source.clone()),
            failed_umount: Some(child.clone()),
        });
        let manager = MountManager::new(Path::new("/run/modama"), Arc::clone(&backend));

        let result = manager.set_state(HashSet::from([
            failed_mount.clone(),
            successful_mount.clone(),
        ]));

        assert!(matches!(
            result,
            Err(SetStateError::Reconciliation(ReconciliationError {
                failed_mounts,
                failed_umounts,
            })) if failed_mounts == vec![failed_mount]
                && failed_umounts == vec![child.clone()]
        ));
        assert_eq!(
            *backend.calls.lock().unwrap(),
            [
                ReconciliationCall::Umount(child),
                ReconciliationCall::Umount(parent),
                ReconciliationCall::Mount(PathBuf::from("/run/modama/shares/new-a")),
                ReconciliationCall::Mount(successful_mount.mount_point),
            ]
        );
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

        fn list_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<PathBuf>, MountBackendError> {
            Ok(HashSet::from([
                PathBuf::from_str("/fail/").unwrap(),
                PathBuf::from_str("/success/").unwrap(),
            ]))
        }

        fn list_stale_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<Vec<PathBuf>, MountBackendError> {
            Ok(Vec::new())
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

        fn list_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<HashSet<PathBuf>, MountBackendError> {
            Ok(HashSet::new())
        }

        fn list_stale_mount_points(
            &self,
            _path_prefix: &Path,
        ) -> Result<Vec<PathBuf>, MountBackendError> {
            Ok(Vec::new())
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
            Err(SetStateError::Reconciliation(ReconciliationError {
                failed_mounts,
                failed_umounts,
            })) => {
                assert_eq!(vec![bm_fail], failed_mounts);
                assert!(failed_umounts.is_empty());
            }
            x => panic!("should not happen: {x:?}"),
        }
    }
}
