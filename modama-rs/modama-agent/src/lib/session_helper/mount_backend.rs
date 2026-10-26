use std::future::Future;

use modama_common::{
    internal_api_types::{MountedShare, SessionHelperConfig},
    messages::Share,
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum MountBackendError {
    #[error("mount failed: {message}")]
    GenericFailure { share_id: String, message: String },
}

pub trait MountBackend {
    /// Try to reconcile mounts. Takes the desired shares, and returns
    /// the state after reconciliation (which might be missing shares,
    /// or still have additional shares - the mount status should
    /// eventually be consistent, but it might take some calls of
    /// this function!)
    fn reconcile(
        &self,
        desired_shares: &[Share],
        config: &SessionHelperConfig,
    ) -> impl Future<Output = Vec<Result<MountedShare, MountBackendError>>>;
}
