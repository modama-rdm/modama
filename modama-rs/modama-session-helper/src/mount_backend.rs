use std::future::Future;

use modama_common::{
    foreground::same_share,
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

/// The outcome of one pass
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassResult {
    /// What exists afterwards; this is what the helper reports
    pub state: SessionHelperState,
    /// Names of the shares this pass mounted; internal to the helper (the mount popup)
    pub created: Vec<String>,
}

pub trait MountBackend {
    /// One reconciliation pass: list what is mounted from `config.smb_hostname`, remove what
    /// isn't desired, mount what's missing, and return the actual state afterwards: every mount
    /// from that host (desired or not), plus an error for each desired share that is still
    /// missing, along with the shares this pass mounted. Mounts should eventually converge, but
    /// it might take some passes.
    ///
    /// With `force`, mounts that are no longer desired are removed even if files are open on
    /// them (the GUI asked to release a previous user's drive).
    fn reconcile(
        &self,
        desired_shares: &[Share],
        config: &SessionHelperConfig,
        force: bool,
    ) -> impl Future<Output = Result<PassResult, EnumerationError>>;
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
