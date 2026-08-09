use modama_common::messages::Share;

use crate::{config::AgentConfig, platform_common::EnsureMountedError, share::MountedShare};

pub trait MountBackend {
    fn initialize(&self, config: &AgentConfig) -> impl std::future::Future<Output = ()> + Send;

    fn cleanup_mount_points(
        &self,
        config: &AgentConfig,
        desired_shares: &[Share],
    ) -> impl std::future::Future<Output = ()> + Send;

    fn ensure_mounted(
        &self,
        share: &Share,
        config: &AgentConfig,
    ) -> impl std::future::Future<Output = Result<MountedShare, EnsureMountedError>> + Send;

    fn umount_old_shares(
        &self,
        config: &AgentConfig,
        shares: &[MountedShare],
    ) -> impl std::future::Future<Output = ()>;
}
