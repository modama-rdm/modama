use std::sync::Arc;

use modama_common::internal_api_types::SessionHelperConfig;
use tokio::sync::Mutex;

use crate::{
    session_helper::helper_state::HelperState, session_helper::mount_backend::MountBackend,
};

/// Inspect the current HelperState and try to make a reconciliation step
/// towards that state, using the given configuration and mount backend.
pub async fn reconcile(
    backend: &impl MountBackend,
    config: &SessionHelperConfig,
    state: &Arc<Mutex<HelperState>>,
) {
    // TODO: error handling
    let desired_shares = state
        .lock()
        .await
        .desired_state
        .as_ref()
        .unwrap()
        .shares
        .clone();

    let mounted_shares = backend.reconcile(&desired_shares, config).await;

    for share in &mounted_shares {
        println!("share mounted: {share:?}");
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn sdasdasd() {}
}
