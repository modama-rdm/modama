use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use futures_util::future::join_all;
use log::{debug, info, warn};
use modama_common::messages::Share;
use nix::mount::MsFlags;
use procfs::{ProcError, process::Process};
use thiserror::Error;

use crate::{
    config::{AgentConfig, AgentLinuxConfig},
    mount_backend::MountBackend,
    platform_common::EnsureMountedError,
    share::MountedShare,
};

async fn prepare_mountpoint_directory(config: &AgentConfig) {
    let prefix = config.get_linux_config().mount_point_prefix;

    if !prefix.exists()
        && let Err(e) = tokio::fs::create_dir_all(prefix).await
    {
        log::error!("could not create mount point directory: {e}");
    }
}

/// Ensure that `share` is mounted, and return a `MountedShare` if the mount succeeded.
async fn ensure_mounted(
    share: &Share,
    config: &AgentConfig,
) -> Result<MountedShare, EnsureMountedError> {
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

    let source = format!("//{}/{}", config.smb_hostname, share.share_name);
    let target = target_for_share(share, config);

    if is_mounted(share, config).map_err(|e| EnsureMountedError::PlatformError(e.into()))? {
        return Ok(MountedShare {
            share: share.clone(),
            mount_path: target,
        });
    }

    let options = format!(
        "domain={},username={},password={},uid={},gid={},forceuid,forcegid,vers=3.1.1,ip={},port={}",
        config.smb_domain,
        share.username,
        share.password,
        linux_config.uid,
        linux_config.gid,
        config.get_smb_ip().await?,
        config.smb_port,
    );

    if !target.exists() {
        std::fs::create_dir_all(&target)?;
    }

    info!(
        "Mounting {} at {} (dest_path={})",
        source,
        target.to_string_lossy(),
        share.dest_path
    );

    let mounted_share = nix::mount::mount::<str, Path, str, str>(
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
    .map_err(|e| EnsureMountedError::PlatformError(e.into()))?;

    info!(
        "Mounted {} at {} (dest_path={})",
        source,
        mounted_share.mount_path.to_string_lossy(),
        mounted_share.share.get_dest_path().to_string_lossy(),
    );

    Ok(mounted_share)
}

#[derive(Error, Debug)]
pub enum MountListError {
    #[error("procfs error: {0}")]
    ProcError(#[from] ProcError),
}

impl From<MountListError> for EnsureMountedError {
    fn from(val: MountListError) -> Self {
        EnsureMountedError::MountFailed(val.to_string())
    }
}

fn is_mounted(share: &Share, config: &AgentConfig) -> Result<bool, MountListError> {
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

fn target_for_share(share: &Share, config: &AgentConfig) -> PathBuf {
    config
        .get_linux_config()
        .mount_point_prefix
        .join(&share.share_name)
}

async fn ensure_umounted(
    mount_point: &Path,
    _config: &AgentConfig,
) -> Result<(), EnsureMountedError> {
    {
        info!("Umounting {}", mount_point.to_string_lossy());
        nix::mount::umount(mount_point).map_err(|e| EnsureMountedError::PlatformError(e.into()))?;
        std::fs::remove_dir(mount_point)?;
        Ok(())
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
        debug!(
            "Could not canonicalize either `{}` or `{}`; falling back to strict equality: {e}",
            m1.display(),
            m2.display()
        );
        m1 == m2
    })
}

fn get_mounts_in_prefix(config: &AgentConfig) -> Result<HashSet<PathBuf>, MountListError> {
    let mounts = Process::myself()?.mountinfo()?;
    Ok(mounts
        .into_iter()
        .map(|m| match m.mount_point.canonicalize() {
            Ok(mp_canonical) => mp_canonical,
            Err(e) => {
                warn!("cannot canonicalize mount point {:?}: {e}", m.mount_point);
                m.mount_point
            }
        })
        .filter(|m| m.starts_with(config.get_linux_config().mount_point_prefix))
        .collect())
}

/// Clean up ingest mount prefix directory,
/// remove all directories that are not currently active mount points.
/// Best-effort; ignores and logs errors and continues to hopefully converge
/// in a clean situation eventually. Don't remove directories that are
/// in the desired-shares list; they are to be used soon.
fn cleanup_mount_points(config: &AgentConfig, desired_shares: &[Share]) {
    let mounts_in_prefix = match get_mounts_in_prefix(config) {
        Ok(m) => m,
        Err(e) => {
            warn!("in cleanup: could not list mounts: {e}");
            return;
        }
    };
    let dir_contents = match std::fs::read_dir(config.get_linux_config().mount_point_prefix) {
        Ok(d) => d,
        Err(e) => {
            warn!("in cleanup: could not read mount point prefix directory: {e}");
            return;
        }
    };

    let desired_share_paths: HashSet<PathBuf> = desired_shares
        .iter()
        .map(|s| target_for_share(s, config))
        .collect();

    dir_contents
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
        .filter(|path| !mounts_in_prefix.contains(path))
        .filter(|path| !desired_share_paths.contains(path))
        .for_each(|path| {
            if let Err(err) = std::fs::remove_dir(&path) {
                warn!("Cannot clean up {}: {err}", path.to_string_lossy());
            } else {
                info!("Cleaned up unused mount point {}", path.to_string_lossy());
            }
        });
}

/// Umount all shares that are no longer supposed to be mounted
async fn umount_old_shares(config: &AgentConfig, shares: &[MountedShare]) {
    let mounts_in_prefix = match get_mounts_in_prefix(config) {
        Ok(m) => m,
        Err(e) => {
            warn!("in cleanup: could not list mounts: {e}");
            return;
        }
    };

    debug!(
        "mounts in prefix: {:?}",
        mounts_in_prefix
            .iter()
            .map(|s| s.to_string_lossy().clone())
            .collect::<Vec<_>>()
    );

    let should_be_mounted: HashSet<PathBuf> = shares.iter().map(|s| s.mount_path.clone()).collect();
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
}

pub struct CurrentPlatformMountBackend {}

impl MountBackend for CurrentPlatformMountBackend {
    async fn initialize(&self, config: &AgentConfig) {
        prepare_mountpoint_directory(config).await;
    }

    async fn cleanup_mount_points(&self, config: &AgentConfig, desired_shares: &[Share]) {
        cleanup_mount_points(config, desired_shares)
    }

    async fn ensure_mounted(
        &self,
        share: &Share,
        config: &AgentConfig,
    ) -> Result<MountedShare, EnsureMountedError> {
        ensure_mounted(share, config).await
    }

    async fn umount_old_shares(&self, config: &AgentConfig, shares: &[MountedShare]) {
        umount_old_shares(config, shares).await
    }
}
