// Daemon-side leader election client.
//
// Acquires a git-leader lease from the relay (TTL 60s), auto-renews on
// heartbeat, and releases on shutdown. Only the leader daemon pushes.
// Communication with the relay is abstracted via `LeaseClient` for testing.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Mutex};
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Lease client trait ──────────────────────────────────────────────

/// Response from a lease acquire request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireResponse {
    /// Lease was granted (new or renewed).
    Granted { lease_id: Uuid },
    /// Lease denied — another daemon holds it.
    Denied { current_holder: Uuid },
}

/// Response from a lease renew request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenewResponse {
    /// Lease renewed successfully.
    Renewed,
    /// Lease not found or expired — need to re-acquire.
    Lost,
}

/// Response from a lease release request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseResponse {
    /// Lease released.
    Released,
    /// No lease to release (already expired or never held).
    NotFound,
}

/// Abstraction over relay lease API calls. Trait-based for testability.
///
/// All methods return `Send` futures so the leader loop can run on
/// a multi-threaded tokio runtime.
pub trait LeaseClient: Send + Sync + 'static {
    /// Try to acquire the git-leader lease for a workspace.
    fn acquire(
        &self,
        workspace_id: Uuid,
        client_id: Uuid,
    ) -> impl std::future::Future<Output = Result<AcquireResponse, LeaseClientError>> + Send;

    /// Renew an existing lease.
    fn renew(
        &self,
        workspace_id: Uuid,
        client_id: Uuid,
        lease_id: Uuid,
    ) -> impl std::future::Future<Output = Result<RenewResponse, LeaseClientError>> + Send;

    /// Release a held lease.
    fn release(
        &self,
        workspace_id: Uuid,
        client_id: Uuid,
    ) -> impl std::future::Future<Output = Result<ReleaseResponse, LeaseClientError>> + Send;
}

/// Errors from the lease client (network or relay-side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseClientError {
    /// Network unreachable / connection refused.
    ConnectionFailed,
    /// Relay returned an unexpected error.
    RelayError { message: String },
}

impl std::fmt::Display for LeaseClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed => write!(f, "lease client: connection failed"),
            Self::RelayError { message } => write!(f, "lease client: relay error: {message}"),
        }
    }
}

impl std::error::Error for LeaseClientError {}

// ── Leader state ────────────────────────────────────────────────────

/// Snapshot of the current leader election state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaderState {
    /// This daemon is the leader.
    Leader { lease_id: Uuid },
    /// Another daemon is the leader.
    Follower,
    /// Haven't acquired or lost the lease; will retry.
    Unknown,
}

impl LeaderState {
    pub fn is_leader(&self) -> bool {
        matches!(self, Self::Leader { .. })
    }
}

// ── Configuration ───────────────────────────────────────────────────

/// Configuration for the leader election loop.
#[derive(Debug, Clone)]
pub struct LeaderConfig {
    pub workspace_id: Uuid,
    pub client_id: Uuid,
    /// How often to renew the lease (should be well under the TTL).
    pub heartbeat_interval: Duration,
    /// How long to wait before retrying after a failed acquire.
    pub retry_interval: Duration,
}

impl LeaderConfig {
    pub fn new(workspace_id: Uuid, client_id: Uuid) -> Self {
        Self {
            workspace_id,
            client_id,
            // Renew at ~40% of TTL (24s out of 60s) for safety margin.
            heartbeat_interval: Duration::from_secs(24),
            retry_interval: Duration::from_secs(5),
        }
    }
}

// ── Leader election loop ────────────────────────────────────────────

/// Runs the leader election loop. Returns a watch receiver for state
/// and a shutdown handle.
pub fn start_leader_election<C: LeaseClient>(
    config: LeaderConfig,
    client: C,
) -> (watch::Receiver<LeaderState>, LeaderHandle<C>) {
    let (state_tx, state_rx) = watch::channel(LeaderState::Unknown);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let inner = Arc::new(LeaderInner {
        config: config.clone(),
        client,
        state_tx,
        current_lease_id: Mutex::new(None),
    });

    let inner_clone = inner.clone();
    let handle = tokio::spawn(async move {
        leader_loop(inner_clone, shutdown_rx).await;
    });

    (state_rx, LeaderHandle { _task: handle, shutdown_tx, inner })
}

/// Handle for the leader election background task.
/// Dropping the handle cancels the task.
pub struct LeaderHandle<C: LeaseClient> {
    _task: tokio::task::JoinHandle<()>,
    shutdown_tx: watch::Sender<bool>,
    inner: Arc<LeaderInner<C>>,
}

impl<C: LeaseClient> LeaderHandle<C> {
    /// Gracefully shut down: release the lease and stop the loop.
    pub async fn shutdown(&self) {
        // Signal the loop to stop.
        let _ = self.shutdown_tx.send(true);

        // Best-effort release.
        let lease_id = self.inner.current_lease_id.lock().await.take();
        if lease_id.is_some() {
            let config = &self.inner.config;
            match self.inner.client.release(config.workspace_id, config.client_id).await {
                Ok(ReleaseResponse::Released) => {
                    info!(workspace = %config.workspace_id, "leader lease released on shutdown");
                }
                Ok(ReleaseResponse::NotFound) => {
                    debug!(workspace = %config.workspace_id, "no lease to release on shutdown");
                }
                Err(e) => {
                    warn!(workspace = %config.workspace_id, error = %e, "failed to release lease on shutdown");
                }
            }
            let _ = self.inner.state_tx.send(LeaderState::Follower);
        }
    }
}

struct LeaderInner<C: LeaseClient> {
    config: LeaderConfig,
    client: C,
    state_tx: watch::Sender<LeaderState>,
    current_lease_id: Mutex<Option<Uuid>>,
}

async fn leader_loop<C: LeaseClient>(
    inner: Arc<LeaderInner<C>>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        let lease_id = *inner.current_lease_id.lock().await;
        let interval = match lease_id {
            Some(lid) => {
                // We hold a lease — renew it.
                match inner
                    .client
                    .renew(inner.config.workspace_id, inner.config.client_id, lid)
                    .await
                {
                    Ok(RenewResponse::Renewed) => {
                        debug!(workspace = %inner.config.workspace_id, "lease renewed");
                        inner.config.heartbeat_interval
                    }
                    Ok(RenewResponse::Lost) => {
                        warn!(workspace = %inner.config.workspace_id, "lease lost during renew");
                        *inner.current_lease_id.lock().await = None;
                        let _ = inner.state_tx.send(LeaderState::Follower);
                        // Immediately try to re-acquire.
                        Duration::ZERO
                    }
                    Err(e) => {
                        warn!(workspace = %inner.config.workspace_id, error = %e, "lease renew failed");
                        // Keep the lease for now — maybe transient network error.
                        // We'll retry on the next heartbeat.
                        inner.config.heartbeat_interval
                    }
                }
            }
            None => {
                // No lease — try to acquire.
                match inner.client.acquire(inner.config.workspace_id, inner.config.client_id).await
                {
                    Ok(AcquireResponse::Granted { lease_id }) => {
                        info!(workspace = %inner.config.workspace_id, %lease_id, "became leader");
                        *inner.current_lease_id.lock().await = Some(lease_id);
                        let _ = inner.state_tx.send(LeaderState::Leader { lease_id });
                        inner.config.heartbeat_interval
                    }
                    Ok(AcquireResponse::Denied { current_holder }) => {
                        debug!(workspace = %inner.config.workspace_id, %current_holder, "lease denied, another daemon is leader");
                        let _ = inner.state_tx.send(LeaderState::Follower);
                        inner.config.retry_interval
                    }
                    Err(e) => {
                        warn!(workspace = %inner.config.workspace_id, error = %e, "lease acquire failed");
                        let _ = inner.state_tx.send(LeaderState::Unknown);
                        inner.config.retry_interval
                    }
                }
            }
        };

        // Sleep or shutdown.
        tokio::select! {
            _ = tokio::time::sleep(interval) => {},
            _ = shutdown_rx.changed() => {
                debug!(workspace = %inner.config.workspace_id, "leader loop shutting down");
                break;
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time;

    /// Test lease client that records calls and returns scripted responses.
    #[derive(Clone)]
    struct MockLeaseClient {
        acquire_responses: Arc<Mutex<Vec<Result<AcquireResponse, LeaseClientError>>>>,
        renew_responses: Arc<Mutex<Vec<Result<RenewResponse, LeaseClientError>>>>,
        release_responses: Arc<Mutex<Vec<Result<ReleaseResponse, LeaseClientError>>>>,
        acquire_count: Arc<AtomicUsize>,
        renew_count: Arc<AtomicUsize>,
        release_count: Arc<AtomicUsize>,
    }

    impl MockLeaseClient {
        fn new() -> Self {
            Self {
                acquire_responses: Arc::new(Mutex::new(Vec::new())),
                renew_responses: Arc::new(Mutex::new(Vec::new())),
                release_responses: Arc::new(Mutex::new(Vec::new())),
                acquire_count: Arc::new(AtomicUsize::new(0)),
                renew_count: Arc::new(AtomicUsize::new(0)),
                release_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        async fn push_acquire(&self, resp: Result<AcquireResponse, LeaseClientError>) {
            self.acquire_responses.lock().await.push(resp);
        }

        async fn push_renew(&self, resp: Result<RenewResponse, LeaseClientError>) {
            self.renew_responses.lock().await.push(resp);
        }

        async fn push_release(&self, resp: Result<ReleaseResponse, LeaseClientError>) {
            self.release_responses.lock().await.push(resp);
        }

        fn acquire_calls(&self) -> usize {
            self.acquire_count.load(Ordering::SeqCst)
        }

        fn renew_calls(&self) -> usize {
            self.renew_count.load(Ordering::SeqCst)
        }

        fn release_calls(&self) -> usize {
            self.release_count.load(Ordering::SeqCst)
        }
    }

    impl LeaseClient for MockLeaseClient {
        async fn acquire(
            &self,
            _workspace_id: Uuid,
            _client_id: Uuid,
        ) -> Result<AcquireResponse, LeaseClientError> {
            self.acquire_count.fetch_add(1, Ordering::SeqCst);
            let mut responses = self.acquire_responses.lock().await;
            if responses.is_empty() {
                // Default: deny with a random holder.
                Ok(AcquireResponse::Denied { current_holder: Uuid::new_v4() })
            } else {
                responses.remove(0)
            }
        }

        async fn renew(
            &self,
            _workspace_id: Uuid,
            _client_id: Uuid,
            _lease_id: Uuid,
        ) -> Result<RenewResponse, LeaseClientError> {
            self.renew_count.fetch_add(1, Ordering::SeqCst);
            let mut responses = self.renew_responses.lock().await;
            if responses.is_empty() {
                Ok(RenewResponse::Renewed)
            } else {
                responses.remove(0)
            }
        }

        async fn release(
            &self,
            _workspace_id: Uuid,
            _client_id: Uuid,
        ) -> Result<ReleaseResponse, LeaseClientError> {
            self.release_count.fetch_add(1, Ordering::SeqCst);
            let mut responses = self.release_responses.lock().await;
            if responses.is_empty() {
                Ok(ReleaseResponse::Released)
            } else {
                responses.remove(0)
            }
        }
    }

    fn test_config() -> LeaderConfig {
        LeaderConfig {
            workspace_id: Uuid::new_v4(),
            client_id: Uuid::new_v4(),
            heartbeat_interval: Duration::from_millis(50),
            retry_interval: Duration::from_millis(20),
        }
    }

    // ── State tests ─────────────────────────────────────────────────

    #[test]
    fn leader_state_is_leader() {
        assert!(LeaderState::Leader { lease_id: Uuid::new_v4() }.is_leader());
        assert!(!LeaderState::Follower.is_leader());
        assert!(!LeaderState::Unknown.is_leader());
    }

    // ── Config tests ────────────────────────────────────────────────

    #[test]
    fn default_config_heartbeat_is_24s() {
        let cfg = LeaderConfig::new(Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(cfg.heartbeat_interval, Duration::from_secs(24));
        assert_eq!(cfg.retry_interval, Duration::from_secs(5));
    }

    // ── LeaseClientError display ────────────────────────────────────

    #[test]
    fn error_display() {
        let e = LeaseClientError::ConnectionFailed;
        assert_eq!(e.to_string(), "lease client: connection failed");

        let e = LeaseClientError::RelayError { message: "timeout".into() };
        assert_eq!(e.to_string(), "lease client: relay error: timeout");
    }

    // ── Election loop integration tests ─────────────────────────────

    #[tokio::test]
    async fn acquires_leadership_on_first_try() {
        time::pause();
        let mock = MockLeaseClient::new();
        let lease_id = Uuid::new_v4();
        mock.push_acquire(Ok(AcquireResponse::Granted { lease_id })).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config, mock.clone());

        // Wait for the state to become Leader.
        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Leader { lease_id });

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn becomes_follower_when_denied() {
        time::pause();
        let mock = MockLeaseClient::new();
        let holder = Uuid::new_v4();
        mock.push_acquire(Ok(AcquireResponse::Denied { current_holder: holder })).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config, mock.clone());

        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Follower);

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn state_unknown_on_connection_failure() {
        time::pause();
        let mock = MockLeaseClient::new();
        mock.push_acquire(Err(LeaseClientError::ConnectionFailed)).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config, mock.clone());

        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Unknown);

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn renews_after_heartbeat_interval() {
        time::pause();
        let mock = MockLeaseClient::new();
        let lease_id = Uuid::new_v4();
        mock.push_acquire(Ok(AcquireResponse::Granted { lease_id })).await;
        mock.push_renew(Ok(RenewResponse::Renewed)).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config.clone(), mock.clone());

        // Wait for acquire.
        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert!(state_rx.borrow().is_leader());

        // Advance past heartbeat interval to trigger renew.
        time::advance(config.heartbeat_interval + Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        // Give the loop a chance to run.
        tokio::time::sleep(Duration::from_millis(1)).await;

        assert_eq!(mock.renew_calls(), 1);
        assert!(state_rx.borrow().is_leader());

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn loses_leadership_on_renew_lost() {
        time::pause();
        let mock = MockLeaseClient::new();
        let lease_id = Uuid::new_v4();
        mock.push_acquire(Ok(AcquireResponse::Granted { lease_id })).await;
        mock.push_renew(Ok(RenewResponse::Lost)).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config.clone(), mock.clone());

        // Acquire.
        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert!(state_rx.borrow().is_leader());

        // Advance past heartbeat — renew returns Lost.
        time::advance(config.heartbeat_interval + Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Follower);

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn retries_acquire_after_denial() {
        time::pause();
        let mock = MockLeaseClient::new();
        let holder = Uuid::new_v4();
        let lease_id = Uuid::new_v4();
        // First deny, then grant on retry.
        mock.push_acquire(Ok(AcquireResponse::Denied { current_holder: holder })).await;
        mock.push_acquire(Ok(AcquireResponse::Granted { lease_id })).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config.clone(), mock.clone());

        // First attempt: denied → Follower.
        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Follower);

        // After retry_interval: second attempt → Leader.
        time::advance(config.retry_interval + Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Leader { lease_id });

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_releases_lease() {
        time::pause();
        let mock = MockLeaseClient::new();
        let lease_id = Uuid::new_v4();
        mock.push_acquire(Ok(AcquireResponse::Granted { lease_id })).await;
        mock.push_release(Ok(ReleaseResponse::Released)).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config, mock.clone());

        // Acquire.
        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert!(state_rx.borrow().is_leader());

        // Shutdown.
        handle.shutdown().await;
        assert_eq!(mock.release_calls(), 1);
        assert_eq!(*state_rx.borrow(), LeaderState::Follower);
    }

    #[tokio::test]
    async fn shutdown_without_lease_does_not_call_release() {
        time::pause();
        let mock = MockLeaseClient::new();
        // Deny acquire — no lease held.
        let holder = Uuid::new_v4();
        mock.push_acquire(Ok(AcquireResponse::Denied { current_holder: holder })).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config, mock.clone());

        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();

        handle.shutdown().await;
        assert_eq!(mock.release_calls(), 0);
    }

    #[tokio::test]
    async fn survives_transient_renew_error() {
        time::pause();
        let mock = MockLeaseClient::new();
        let lease_id = Uuid::new_v4();
        mock.push_acquire(Ok(AcquireResponse::Granted { lease_id })).await;
        // First renew fails (transient), second succeeds.
        mock.push_renew(Err(LeaseClientError::ConnectionFailed)).await;
        mock.push_renew(Ok(RenewResponse::Renewed)).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config.clone(), mock.clone());

        // Acquire.
        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert!(state_rx.borrow().is_leader());

        // First renew: error, but we stay leader.
        time::advance(config.heartbeat_interval + Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert!(state_rx.borrow().is_leader());

        // Second renew: success, still leader.
        time::advance(config.heartbeat_interval + Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert!(state_rx.borrow().is_leader());
        assert_eq!(mock.renew_calls(), 2);

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn re_acquires_after_lease_lost() {
        time::pause();
        let mock = MockLeaseClient::new();
        let lease1 = Uuid::new_v4();
        let lease2 = Uuid::new_v4();
        // Acquire → renew lost → re-acquire.
        mock.push_acquire(Ok(AcquireResponse::Granted { lease_id: lease1 })).await;
        mock.push_renew(Ok(RenewResponse::Lost)).await;
        mock.push_acquire(Ok(AcquireResponse::Granted { lease_id: lease2 })).await;

        let config = test_config();
        let (mut state_rx, handle) = start_leader_election(config.clone(), mock.clone());

        // Acquire first lease.
        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Leader { lease_id: lease1 });

        // Renew → lost → becomes follower.
        time::advance(config.heartbeat_interval + Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Follower);

        // Immediately re-acquires (Duration::ZERO wait after loss).
        // Give it time to process (the zero-duration sleep still yields).
        time::advance(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
        state_rx.changed().await.unwrap();
        assert_eq!(*state_rx.borrow(), LeaderState::Leader { lease_id: lease2 });

        assert_eq!(mock.acquire_calls(), 2);

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn acquire_count_increases_on_retries() {
        time::pause();
        let mock = MockLeaseClient::new();
        let holder = Uuid::new_v4();
        // Three consecutive denials.
        mock.push_acquire(Ok(AcquireResponse::Denied { current_holder: holder })).await;
        mock.push_acquire(Ok(AcquireResponse::Denied { current_holder: holder })).await;
        mock.push_acquire(Ok(AcquireResponse::Denied { current_holder: holder })).await;

        let config = test_config();
        let (_state_rx, handle) = start_leader_election(config.clone(), mock.clone());

        // Advance through 3 retry cycles.
        for _ in 0..3 {
            time::advance(config.retry_interval + Duration::from_millis(10)).await;
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        assert!(mock.acquire_calls() >= 3);

        handle.shutdown().await;
    }
}
