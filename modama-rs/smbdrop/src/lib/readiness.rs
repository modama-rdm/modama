use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    str::FromStr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use log::debug;
use modama_common::messages::Share;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{watch, Mutex, Semaphore},
    time::{sleep, timeout, Instant},
};

const MAX_CONCURRENT_PROBES: usize = 30;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const STALE_CHECK_AFTER: Duration = Duration::from_secs(30);
const RETRY_INTERVAL: Duration = Duration::from_millis(500);
const CREATE_READINESS_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ShareReadinessError {
    #[error("SMB share is not ready before the deadline: {0}")]
    Timeout(String),

    #[error("SMB readiness check failed: {0}")]
    Check(String),
}

pub type ShareProbeFuture = Pin<Box<dyn Future<Output = Result<(), ShareReadinessError>> + Send>>;

pub trait ShareProbe: Send + Sync {
    fn check(&self, share: Share) -> ShareProbeFuture;
}

#[derive(Debug, Clone)]
pub struct SmbShareProbe {
    server: String,
}

impl Default for SmbShareProbe {
    fn default() -> Self {
        Self {
            server: "127.0.0.1".to_owned(),
        }
    }
}

impl ShareProbe for SmbShareProbe {
    fn check(&self, share: Share) -> ShareProbeFuture {
        let server = self.server.clone();
        Box::pin(async move {
            let client = smb::Client::new(smb::ClientConfig::default());
            let target = smb::UncPath::from_str(&format!(r"\\{}\{}", server, share.share_name))
                .map_err(|e| ShareReadinessError::Check(e.to_string()))?;

            let result = client
                .share_connect(&target, &share.username, share.password)
                .await
                .map_err(|e| ShareReadinessError::Check(e.to_string()));
            client
                .close()
                .await
                .map_err(|e| ShareReadinessError::Check(e.to_string()))?;
            result
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoopShareProbe;

impl ShareProbe for NoopShareProbe {
    fn check(&self, _share: Share) -> ShareProbeFuture {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug, Clone, Copy)]
enum CacheEntry {
    Ready,
    Checking { started_at: Instant, token: u64 },
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ShareReadinessCounts {
    pub total: usize,
    pub ready: usize,
    pub checking: usize,
    pub unknown: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GlobalReadinessStatus {
    Ready,
    Checking,
    NotReady,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GlobalReadinessReason {
    NoShares,
    ShareReady,
    ProbeInProgress,
    NoShareReady,
    StateUnavailable,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct GlobalReadiness {
    pub status: GlobalReadinessStatus,
    pub reason: GlobalReadinessReason,
    pub shares: ShareReadinessCounts,
}

struct CacheInner {
    entries: Mutex<HashMap<String, CacheEntry>>,
    probe: Arc<dyn ShareProbe>,
    permits: Arc<Semaphore>,
    revision_tx: watch::Sender<u64>,
    next_token: AtomicU64,
    probe_timeout: Duration,
    stale_check_after: Duration,
    retry_interval: Duration,
}

#[derive(Clone)]
pub struct ShareReadinessCache {
    inner: Arc<CacheInner>,
}

impl Default for ShareReadinessCache {
    fn default() -> Self {
        Self::new(Arc::new(SmbShareProbe::default()))
    }
}

impl ShareReadinessCache {
    pub fn new(probe: Arc<dyn ShareProbe>) -> Self {
        Self::with_config(
            probe,
            MAX_CONCURRENT_PROBES,
            PROBE_TIMEOUT,
            STALE_CHECK_AFTER,
            RETRY_INTERVAL,
        )
    }

    fn with_config(
        probe: Arc<dyn ShareProbe>,
        max_concurrent_probes: usize,
        probe_timeout: Duration,
        stale_check_after: Duration,
        retry_interval: Duration,
    ) -> Self {
        let (revision_tx, _) = watch::channel(0);
        Self {
            inner: Arc::new(CacheInner {
                entries: Mutex::new(HashMap::new()),
                probe,
                permits: Arc::new(Semaphore::new(max_concurrent_probes)),
                revision_tx,
                next_token: AtomicU64::new(0),
                probe_timeout,
                stale_check_after,
                retry_interval,
            }),
        }
    }

    fn signal_change(&self) {
        self.inner
            .revision_tx
            .send_modify(|revision| *revision += 1);
    }

    pub async fn invalidate(&self) {
        self.inner.entries.lock().await.clear();
        self.signal_change();
    }

    pub async fn mark_ready(&self, share_id: &str) {
        self.inner
            .entries
            .lock()
            .await
            .insert(share_id.to_owned(), CacheEntry::Ready);
        self.signal_change();
    }

    pub async fn snapshot(&self, shares: &[Share]) -> ShareReadinessCounts {
        let entries = self.inner.entries.lock().await;
        let now = Instant::now();
        let mut counts = ShareReadinessCounts {
            total: shares.len(),
            ..ShareReadinessCounts::default()
        };

        for share in shares {
            match entries.get(&share.share_id) {
                Some(CacheEntry::Ready) => counts.ready += 1,
                Some(CacheEntry::Checking { started_at, .. })
                    if now.duration_since(*started_at) < self.inner.stale_check_after =>
                {
                    counts.checking += 1;
                }
                Some(CacheEntry::Checking { .. }) | None => counts.unknown += 1,
            }
        }

        counts
    }

    pub async fn global_readiness(&self, shares: &[Share]) -> GlobalReadiness {
        let mut counts = self.snapshot(shares).await;
        if counts.total == 0 {
            return GlobalReadiness {
                status: GlobalReadinessStatus::Ready,
                reason: GlobalReadinessReason::NoShares,
                shares: counts,
            };
        }
        if counts.ready > 0 {
            return GlobalReadiness {
                status: GlobalReadinessStatus::Ready,
                reason: GlobalReadinessReason::ShareReady,
                shares: counts,
            };
        }

        let health_window = self
            .inner
            .probe_timeout
            .saturating_sub(Duration::from_millis(1));
        let _ = timeout(health_window, self.wait_until_any_ready(shares)).await;
        counts = self.snapshot(shares).await;

        if counts.ready > 0 {
            GlobalReadiness {
                status: GlobalReadinessStatus::Ready,
                reason: GlobalReadinessReason::ShareReady,
                shares: counts,
            }
        } else if counts.checking > 0 {
            GlobalReadiness {
                status: GlobalReadinessStatus::Checking,
                reason: GlobalReadinessReason::ProbeInProgress,
                shares: counts,
            }
        } else {
            GlobalReadiness {
                status: GlobalReadinessStatus::NotReady,
                reason: GlobalReadinessReason::NoShareReady,
                shares: counts,
            }
        }
    }

    pub async fn wait_until_ready(&self, share: Share) -> Result<(), ShareReadinessError> {
        let deadline = Instant::now() + CREATE_READINESS_DEADLINE;
        let mut last_error = None;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ShareReadinessError::Timeout(
                    last_error.unwrap_or_else(|| share.share_name.clone()),
                ));
            }

            let permit =
                match timeout(remaining, Arc::clone(&self.inner.permits).acquire_owned()).await {
                    Ok(Ok(permit)) => permit,
                    Ok(Err(error)) => return Err(ShareReadinessError::Check(error.to_string())),
                    Err(_) => {
                        return Err(ShareReadinessError::Timeout(
                            last_error.unwrap_or_else(|| share.share_name.clone()),
                        ));
                    }
                };
            let remaining = deadline.saturating_duration_since(Instant::now());
            let attempt_timeout = self.inner.probe_timeout.min(remaining);
            match timeout(attempt_timeout, self.inner.probe.check(share.clone())).await {
                Ok(Ok(())) => {
                    drop(permit);
                    self.mark_ready(&share.share_id).await;
                    return Ok(());
                }
                Ok(Err(error)) => last_error = Some(error.to_string()),
                Err(_) => last_error = Some(format!("probe timed out for {}", share.share_name)),
            }
            drop(permit);

            sleep(self.inner.retry_interval.min(remaining)).await;
        }
    }

    pub async fn wait_until_any_ready(&self, shares: &[Share]) {
        if shares.is_empty() {
            return;
        }

        let mut revision_rx = self.inner.revision_tx.subscribe();
        let mut cursor = 0;

        loop {
            let now = Instant::now();
            let mut entries = self.inner.entries.lock().await;

            for share in shares {
                if matches!(entries.get(&share.share_id), Some(CacheEntry::Ready)) {
                    return;
                }
            }

            for share in shares {
                let is_stale = matches!(
                    entries.get(&share.share_id),
                    Some(CacheEntry::Checking { started_at, .. })
                        if now.duration_since(*started_at) >= self.inner.stale_check_after
                );
                if is_stale {
                    entries.remove(&share.share_id);
                }
            }

            let mut probes = Vec::new();
            for offset in 0..shares.len() {
                let index = (cursor + offset) % shares.len();
                let share = &shares[index];
                if entries.contains_key(&share.share_id) {
                    continue;
                }

                let Ok(permit) = Arc::clone(&self.inner.permits).try_acquire_owned() else {
                    break;
                };
                let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed);
                entries.insert(
                    share.share_id.clone(),
                    CacheEntry::Checking {
                        started_at: now,
                        token,
                    },
                );
                probes.push((share.clone(), token, permit));
                cursor = (index + 1) % shares.len();
            }
            drop(entries);

            for (share, token, permit) in probes {
                let cache = self.clone();
                tokio::spawn(async move {
                    let result = timeout(
                        cache.inner.probe_timeout,
                        cache.inner.probe.check(share.clone()),
                    )
                    .await;
                    drop(permit);
                    cache.finish_probe(&share, token, result).await;
                });
            }

            tokio::select! {
                changed = revision_rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let entries = self.inner.entries.lock().await;
                    if shares.iter().any(|share| {
                        matches!(entries.get(&share.share_id), Some(CacheEntry::Ready))
                    }) {
                        return;
                    }
                    drop(entries);
                    sleep(self.inner.retry_interval).await;
                }
                _ = sleep(self.inner.retry_interval) => {}
            }
        }
    }

    async fn finish_probe(
        &self,
        share: &Share,
        token: u64,
        result: Result<Result<(), ShareReadinessError>, tokio::time::error::Elapsed>,
    ) {
        let mut entries = self.inner.entries.lock().await;
        let owns_entry = matches!(
            entries.get(&share.share_id),
            Some(CacheEntry::Checking {
                token: entry_token,
                ..
            }) if *entry_token == token
        );
        if !owns_entry {
            return;
        }

        match result {
            Ok(Ok(())) => {
                entries.insert(share.share_id.clone(), CacheEntry::Ready);
            }
            Ok(Err(error)) => {
                debug!(
                    "SMB readiness probe failed for {}: {error}",
                    share.share_name
                );
                entries.remove(&share.share_id);
            }
            Err(_) => {
                debug!("SMB readiness probe timed out for {}", share.share_name);
                entries.remove(&share.share_id);
            }
        }
        drop(entries);
        self.signal_change();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::pending,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use tokio::sync::Notify;

    use super::*;

    struct CountingProbe {
        calls: AtomicUsize,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
        fail_before: usize,
        release: Option<Arc<Notify>>,
    }

    impl CountingProbe {
        fn new(fail_before: usize, release: Option<Arc<Notify>>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
                fail_before,
                release,
            }
        }
    }

    impl ShareProbe for Arc<CountingProbe> {
        fn check(&self, _share: Share) -> ShareProbeFuture {
            let probe = Arc::clone(self);
            Box::pin(async move {
                let call = probe.calls.fetch_add(1, Ordering::SeqCst) + 1;
                let in_flight = probe.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                probe.max_in_flight.fetch_max(in_flight, Ordering::SeqCst);
                if let Some(release) = &probe.release {
                    release.notified().await;
                }
                probe.in_flight.fetch_sub(1, Ordering::SeqCst);

                if call <= probe.fail_before {
                    Err(ShareReadinessError::Check("not ready".to_owned()))
                } else {
                    Ok(())
                }
            })
        }
    }

    struct StallFirstProbe {
        calls: AtomicUsize,
    }

    impl ShareProbe for Arc<StallFirstProbe> {
        fn check(&self, _share: Share) -> ShareProbeFuture {
            let probe = Arc::clone(self);
            Box::pin(async move {
                if probe.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    pending::<()>().await;
                }
                Ok(())
            })
        }
    }

    fn share(index: usize) -> Share {
        Share {
            resource: Some("resource".to_owned()),
            share_id: format!("share-id-{index}"),
            share_name: format!("share-{index}"),
            dest_path: "/data".to_owned(),
            created_timestamp: 0,
            username: "user".to_owned(),
            password: "password".to_owned(),
            uid: 1000,
            gid: 1000,
        }
    }

    #[tokio::test]
    async fn empty_share_list_is_ready() {
        let cache = ShareReadinessCache::new(Arc::new(NoopShareProbe));
        cache.wait_until_any_ready(&[]).await;
    }

    #[tokio::test]
    async fn global_readiness_is_immediate_without_shares() {
        let cache = ShareReadinessCache::new(Arc::new(NoopShareProbe));
        assert_eq!(
            cache.global_readiness(&[]).await,
            GlobalReadiness {
                status: GlobalReadinessStatus::Ready,
                reason: GlobalReadinessReason::NoShares,
                shares: ShareReadinessCounts::default(),
            }
        );
    }

    #[tokio::test]
    async fn global_readiness_probes_an_unknown_share() {
        let cache = ShareReadinessCache::new(Arc::new(NoopShareProbe));
        assert_eq!(
            cache.global_readiness(&[share(1)]).await,
            GlobalReadiness {
                status: GlobalReadinessStatus::Ready,
                reason: GlobalReadinessReason::ShareReady,
                shares: ShareReadinessCounts {
                    total: 1,
                    ready: 1,
                    checking: 0,
                    unknown: 0,
                },
            }
        );
    }

    #[tokio::test]
    async fn global_readiness_reports_an_active_probe() {
        let release = Arc::new(Notify::new());
        let probe = Arc::new(CountingProbe::new(0, Some(release)));
        let cache = ShareReadinessCache::with_config(
            Arc::new(Arc::clone(&probe)),
            30,
            Duration::from_millis(20),
            Duration::from_secs(1),
            Duration::from_millis(1),
        );

        let readiness = cache.global_readiness(&[share(1)]).await;
        assert_eq!(readiness.status, GlobalReadinessStatus::Checking);
        assert_eq!(readiness.reason, GlobalReadinessReason::ProbeInProgress);
        assert_eq!(readiness.shares.checking, 1);
    }

    #[tokio::test]
    async fn global_readiness_reports_failed_probes() {
        let probe = Arc::new(CountingProbe::new(usize::MAX, None));
        let cache = ShareReadinessCache::with_config(
            Arc::new(Arc::clone(&probe)),
            30,
            Duration::from_millis(10),
            Duration::from_secs(1),
            Duration::from_millis(100),
        );

        let readiness = cache.global_readiness(&[share(1)]).await;
        assert_eq!(readiness.status, GlobalReadinessStatus::NotReady);
        assert_eq!(readiness.reason, GlobalReadinessReason::NoShareReady);
        assert_eq!(readiness.shares.unknown, 1);
    }

    #[tokio::test]
    async fn failed_probe_retries_until_one_succeeds() {
        let probe = Arc::new(CountingProbe::new(2, None));
        let cache = ShareReadinessCache::with_config(
            Arc::new(Arc::clone(&probe)),
            30,
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_millis(1),
        );

        timeout(
            Duration::from_secs(1),
            cache.wait_until_any_ready(&[share(1)]),
        )
        .await
        .expect("readiness should recover");
        assert_eq!(probe.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn concurrent_waiters_share_one_probe() {
        let release = Arc::new(Notify::new());
        let probe = Arc::new(CountingProbe::new(0, Some(Arc::clone(&release))));
        let cache = Arc::new(ShareReadinessCache::new(Arc::new(Arc::clone(&probe))));
        let target = share(1);
        let mut waiters = Vec::new();
        for _ in 0..20 {
            let cache = Arc::clone(&cache);
            let target = target.clone();
            waiters.push(tokio::spawn(async move {
                cache.wait_until_any_ready(&[target]).await;
            }));
        }

        timeout(Duration::from_secs(1), async {
            while probe.calls.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        release.notify_waiters();
        for waiter in waiters {
            waiter.await.unwrap();
        }
    }

    #[tokio::test]
    async fn global_probe_limit_is_enforced() {
        let release = Arc::new(Notify::new());
        let probe = Arc::new(CountingProbe::new(0, Some(Arc::clone(&release))));
        let cache = Arc::new(ShareReadinessCache::new(Arc::new(Arc::clone(&probe))));
        let shares = (0..60).map(share).collect::<Vec<_>>();
        let waiter = {
            let cache = Arc::clone(&cache);
            tokio::spawn(async move { cache.wait_until_any_ready(&shares).await })
        };

        timeout(Duration::from_secs(1), async {
            while probe.calls.load(Ordering::SeqCst) < MAX_CONCURRENT_PROBES {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            probe.max_in_flight.load(Ordering::SeqCst),
            MAX_CONCURRENT_PROBES
        );
        release.notify_waiters();
        waiter.await.unwrap();
    }

    #[tokio::test]
    async fn invalidation_forces_a_fresh_probe() {
        let probe = Arc::new(CountingProbe::new(0, None));
        let cache = ShareReadinessCache::new(Arc::new(Arc::clone(&probe)));
        let target = share(1);

        cache
            .wait_until_any_ready(std::slice::from_ref(&target))
            .await;
        cache
            .wait_until_any_ready(std::slice::from_ref(&target))
            .await;
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);

        cache.invalidate().await;
        cache.wait_until_any_ready(&[target]).await;
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stale_check_is_reclaimed() {
        let probe = Arc::new(StallFirstProbe {
            calls: AtomicUsize::new(0),
        });
        let cache = ShareReadinessCache::with_config(
            Arc::new(Arc::clone(&probe)),
            2,
            Duration::from_secs(1),
            Duration::from_millis(10),
            Duration::from_millis(1),
        );

        timeout(
            Duration::from_secs(1),
            cache.wait_until_any_ready(&[share(1)]),
        )
        .await
        .expect("stale probe should be replaced");
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
    }
}
