use modama_common::{
    internal_api_types::{MountedShare, SessionHelperConfig, SessionHelperState},
    messages::Share,
};

use crate::session_helper::mount_backend::{EnumerationError, MountBackend};

pub struct CurrentPlatformMountBackend {}

impl CurrentPlatformMountBackend {
    pub async fn new() -> Self {
        todo!();
    }
}

impl MountBackend for CurrentPlatformMountBackend {
    async fn reconcile(
        &self,
        desired_shares: &[Share],
        config: &SessionHelperConfig,
    ) -> Result<SessionHelperState, EnumerationError> {
        let actual_mounts_before: Vec<MountedShare> = todo!("current mounts");

        todo!("identify and unmount unwanted mounts");

        todo!("identify missing mounts, mount them");

        let actual_mounts_after: Vec<MountedShare> = todo!("current mounts");

        todo!(
            "SessionHelperState with _all_ actual_mounts_after, and errors for desired mounts that \
             we couldn't mount"
        );
    }
}
