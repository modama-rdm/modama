use std::{future::Future, pin::Pin, time::Duration};

use log::debug;
use modama_common::messages::Share;
use tokio::time::{sleep, timeout, Instant};

const READINESS_DEADLINE: Duration = Duration::from_secs(30);
const READINESS_RETRY_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ShareReadinessError {
    #[error("SMB share is not ready before the deadline: {0}")]
    Timeout(String),

    #[error("SMB readiness check failed: {0}")]
    Check(String),
}

pub type ShareReadinessFuture =
    Pin<Box<dyn Future<Output = Result<(), ShareReadinessError>> + Send>>;

pub trait ShareReadiness: Send + Sync {
    fn wait_until_ready(&self, share: Share) -> ShareReadinessFuture;
    fn check_once(&self, share: Share) -> ShareReadinessFuture;
}

#[derive(Debug, Clone)]
pub struct SmbShareReadiness {
    server: String,
}

impl Default for SmbShareReadiness {
    fn default() -> Self {
        Self {
            server: "127.0.0.1".to_owned(),
        }
    }
}

impl From<smb2::Error> for ShareReadinessError {
    fn from(value: smb2::Error) -> Self {
        ShareReadinessError::Check(value.to_string())
    }
}

impl ShareReadiness for SmbShareReadiness {
    fn wait_until_ready(&self, share: Share) -> ShareReadinessFuture {
        let checker = self.clone();
        Box::pin(async move {
            let deadline = Instant::now() + READINESS_DEADLINE;
            let mut last_error = None;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(ShareReadinessError::Timeout(
                        last_error.unwrap_or_else(|| share.share_name.clone()),
                    ));
                }

                match timeout(remaining, checker.check_once(share.clone())).await {
                    Ok(Ok(())) => return Ok(()),
                    Ok(Err(error)) => {
                        last_error = Some(error.to_string());
                        debug!(
                            "readiness probe failed for share {}: {error}",
                            share.share_name
                        );
                    }
                    Err(_) => return Err(ShareReadinessError::Timeout(share.share_name.clone())),
                }

                sleep(READINESS_RETRY_INTERVAL.min(remaining)).await;
            }
        })
    }

    fn check_once(&self, share: Share) -> ShareReadinessFuture {
        let server = format!("{}:445", self.server);
        Box::pin(async move {
            let mut conn = smb2::connect(&server, &share.username, &share.password).await?;
            let tree = conn.connect_share(&share.share_name).await?;
            conn.disconnect_share(&tree).await?;
            Ok(())
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoopShareReadiness;

impl ShareReadiness for NoopShareReadiness {
    fn wait_until_ready(&self, _share: Share) -> ShareReadinessFuture {
        Box::pin(async { Ok(()) })
    }

    fn check_once(&self, _share: Share) -> ShareReadinessFuture {
        Box::pin(async { Ok(()) })
    }
}
