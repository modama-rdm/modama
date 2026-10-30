use std::future::Future;

use modama_common::{
    internal_api_types::{SessionHelperConfig, SessionHelperState, ShareError},
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

/// SMB share names are case-insensitive
pub fn same_share(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The desired shares, without those whose name another one already has; each of those gets an
/// error instead
pub fn unique_by_name(shares: &[Share]) -> (Vec<&Share>, Vec<ShareError>) {
    let mut unique: Vec<&Share> = Vec::new();
    let mut errors = Vec::new();
    for share in shares {
        if unique
            .iter()
            .any(|u| same_share(&u.share_name, &share.share_name))
        {
            errors.push(ShareError {
                share_id: share.share_id.clone(),
                message: "another desired share has the same name".to_owned(),
            });
        } else {
            unique.push(share);
        }
    }
    (unique, errors)
}
