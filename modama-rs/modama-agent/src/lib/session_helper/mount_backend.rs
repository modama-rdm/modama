use std::future::Future;

use modama_common::{
    internal_api_types::{SessionHelperConfig, SessionHelperState},
    messages::Share,
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum MountBackendError {
    #[error("mount failed: {message}")]
    GenericFailure { share_id: String, message: String },
}

/// The backend couldn't determine what is mounted; nothing must be reported
#[derive(Debug, thiserror::Error)]
#[error("could not list mounts: {0}")]
pub struct EnumerationError(pub String);

pub trait MountBackend {
    /// One reconciliation pass: list what is mounted from `config.smb_hostname`, remove what
    /// isn't desired, mount what's missing, and return the actual state afterwards: every mount
    /// from that host (desired or not), plus an error for each desired share that is still
    /// missing. Mounts should eventually converge, but it might take some passes.
    fn reconcile(
        &self,
        desired_shares: &[Share],
        config: &SessionHelperConfig,
    ) -> impl Future<Output = Result<SessionHelperState, EnumerationError>>;
}
