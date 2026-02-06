// Git leader-election lease coordination.
//
// One daemon per workspace holds the git-push lease at a time.
// Lease-based: TTL 60s, auto-renew on heartbeat, one leader per workspace.
// Daemons acquire leases via the relay; the relay manages lease state.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use uuid::Uuid;

/// Default lease duration.
pub const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(60);

/// A granted lease for a workspace's git operations.
#[derive(Debug, Clone)]
pub struct Lease {
    /// Workspace this lease is for.
    pub workspace_id: Uuid,
    /// Client (daemon) that holds the lease.
    pub holder_id: Uuid,
    /// Unique lease identifier (for release/renew).
    pub lease_id: Uuid,
    /// When the lease was granted.
    pub granted_at: Instant,
    /// When the lease expires (unless renewed).
    pub expires_at: Instant,
}

impl Lease {
    /// Whether this lease has expired.
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(Instant::now())
    }

    fn is_expired_at(&self, now: Instant) -> bool {
        now >= self.expires_at
    }

    /// Remaining time on the lease.
    pub fn remaining(&self) -> Duration {
        let now = Instant::now();
        if now >= self.expires_at {
            Duration::ZERO
        } else {
            self.expires_at - now
        }
    }
}

/// Result of a lease acquisition attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireResult {
    /// Lease granted.
    Granted { lease_id: Uuid },
    /// Lease denied — another daemon holds it.
    Denied { current_holder: Uuid },
    /// Lease re-granted (caller already held it and it was renewed).
    Renewed { lease_id: Uuid },
}

/// Result of a lease renewal attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenewResult {
    /// Lease renewed successfully.
    Renewed,
    /// Lease not found or expired.
    NotFound,
    /// Wrong holder — lease belongs to someone else.
    WrongHolder,
}

/// Result of a lease release attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseResult {
    /// Lease released successfully.
    Released,
    /// No active lease for this workspace.
    NotFound,
    /// Wrong holder — cannot release someone else's lease.
    WrongHolder,
}

/// Manages git leader-election leases for workspaces.
///
/// Thread-safe: expects external synchronization (e.g. Mutex).
pub struct LeaseManager {
    leases: HashMap<Uuid, Lease>,
    ttl: Duration,
}

impl LeaseManager {
    pub fn new() -> Self {
        Self { leases: HashMap::new(), ttl: DEFAULT_LEASE_TTL }
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self { leases: HashMap::new(), ttl }
    }

    /// Try to acquire the git leader lease for a workspace.
    ///
    /// - If no active lease exists, grants a new one.
    /// - If the caller already holds the lease, renews it.
    /// - If another daemon holds an active lease, denies.
    pub fn acquire(&mut self, workspace_id: Uuid, client_id: Uuid) -> AcquireResult {
        self.acquire_at(workspace_id, client_id, Instant::now())
    }

    fn acquire_at(
        &mut self,
        workspace_id: Uuid,
        client_id: Uuid,
        now: Instant,
    ) -> AcquireResult {
        // Check existing lease.
        if let Some(lease) = self.leases.get_mut(&workspace_id) {
            if !lease.is_expired_at(now) {
                if lease.holder_id == client_id {
                    // Re-acquire = renew.
                    lease.expires_at = now + self.ttl;
                    return AcquireResult::Renewed { lease_id: lease.lease_id };
                } else {
                    // Active lease held by someone else.
                    return AcquireResult::Denied { current_holder: lease.holder_id };
                }
            }
            // Expired — remove and grant new.
        }

        // Grant new lease.
        let lease_id = Uuid::new_v4();
        let lease = Lease {
            workspace_id,
            holder_id: client_id,
            lease_id,
            granted_at: now,
            expires_at: now + self.ttl,
        };
        self.leases.insert(workspace_id, lease);

        AcquireResult::Granted { lease_id }
    }

    /// Renew an existing lease (heartbeat).
    pub fn renew(&mut self, workspace_id: Uuid, client_id: Uuid, lease_id: Uuid) -> RenewResult {
        self.renew_at(workspace_id, client_id, lease_id, Instant::now())
    }

    fn renew_at(
        &mut self,
        workspace_id: Uuid,
        client_id: Uuid,
        lease_id: Uuid,
        now: Instant,
    ) -> RenewResult {
        let Some(lease) = self.leases.get_mut(&workspace_id) else {
            return RenewResult::NotFound;
        };

        if lease.is_expired_at(now) {
            self.leases.remove(&workspace_id);
            return RenewResult::NotFound;
        }

        if lease.lease_id != lease_id || lease.holder_id != client_id {
            return RenewResult::WrongHolder;
        }

        lease.expires_at = now + self.ttl;
        RenewResult::Renewed
    }

    /// Release a lease voluntarily (daemon shutting down or done pushing).
    pub fn release(&mut self, workspace_id: Uuid, client_id: Uuid) -> ReleaseResult {
        self.release_at(workspace_id, client_id, Instant::now())
    }

    fn release_at(
        &mut self,
        workspace_id: Uuid,
        client_id: Uuid,
        now: Instant,
    ) -> ReleaseResult {
        let Some(lease) = self.leases.get(&workspace_id) else {
            return ReleaseResult::NotFound;
        };

        if lease.is_expired_at(now) {
            self.leases.remove(&workspace_id);
            return ReleaseResult::NotFound;
        }

        if lease.holder_id != client_id {
            return ReleaseResult::WrongHolder;
        }

        self.leases.remove(&workspace_id);
        ReleaseResult::Released
    }

    /// Get the current leader for a workspace (if any active lease exists).
    pub fn current_leader(&self, workspace_id: Uuid) -> Option<Uuid> {
        self.current_leader_at(workspace_id, Instant::now())
    }

    fn current_leader_at(&self, workspace_id: Uuid, now: Instant) -> Option<Uuid> {
        self.leases
            .get(&workspace_id)
            .filter(|lease| !lease.is_expired_at(now))
            .map(|lease| lease.holder_id)
    }

    /// Evict all expired leases. Returns the count of evicted leases.
    pub fn evict_expired(&mut self) -> usize {
        self.evict_expired_at(Instant::now())
    }

    fn evict_expired_at(&mut self, now: Instant) -> usize {
        let before = self.leases.len();
        self.leases.retain(|_, lease| !lease.is_expired_at(now));
        before - self.leases.len()
    }

    /// Number of active (non-expired) leases.
    pub fn active_count(&self) -> usize {
        self.active_count_at(Instant::now())
    }

    fn active_count_at(&self, now: Instant) -> usize {
        self.leases.values().filter(|l| !l.is_expired_at(now)).count()
    }
}

impl Default for LeaseManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use uuid::Uuid;

    use super::*;

    fn ids() -> (Uuid, Uuid, Uuid) {
        (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4())
    }

    // ── Acquire ────────────────────────────────────────────────────

    #[test]
    fn acquire_grants_when_no_lease() {
        let mut mgr = LeaseManager::new();
        let (ws, client, _) = ids();

        let result = mgr.acquire(ws, client);
        assert!(matches!(result, AcquireResult::Granted { .. }));
    }

    #[test]
    fn acquire_denies_when_another_holds() {
        let mut mgr = LeaseManager::new();
        let (ws, client1, client2) = ids();

        mgr.acquire(ws, client1);
        let result = mgr.acquire(ws, client2);

        match result {
            AcquireResult::Denied { current_holder } => {
                assert_eq!(current_holder, client1);
            }
            _ => panic!("expected Denied"),
        }
    }

    #[test]
    fn acquire_renews_when_same_holder() {
        let mut mgr = LeaseManager::new();
        let (ws, client, _) = ids();

        let first = mgr.acquire(ws, client);
        let lease_id = match first {
            AcquireResult::Granted { lease_id } => lease_id,
            _ => panic!("expected Granted"),
        };

        let second = mgr.acquire(ws, client);
        match second {
            AcquireResult::Renewed { lease_id: renewed_id } => {
                assert_eq!(renewed_id, lease_id);
            }
            _ => panic!("expected Renewed"),
        }
    }

    #[test]
    fn acquire_grants_after_expiry() {
        let mut mgr = LeaseManager::with_ttl(Duration::from_secs(1));
        let (ws, client1, client2) = ids();

        let now = Instant::now();
        mgr.acquire_at(ws, client1, now);

        // After TTL expires, another client can acquire.
        let later = now + Duration::from_secs(2);
        let result = mgr.acquire_at(ws, client2, later);
        assert!(matches!(result, AcquireResult::Granted { .. }));
    }

    // ── Renew ──────────────────────────────────────────────────────

    #[test]
    fn renew_extends_lease() {
        let mut mgr = LeaseManager::with_ttl(Duration::from_secs(5));
        let (ws, client, _) = ids();

        let now = Instant::now();
        let lease_id = match mgr.acquire_at(ws, client, now) {
            AcquireResult::Granted { lease_id } => lease_id,
            _ => panic!("expected Granted"),
        };

        // Renew at 3s — should extend to 8s from start.
        let at_3s = now + Duration::from_secs(3);
        assert_eq!(mgr.renew_at(ws, client, lease_id, at_3s), RenewResult::Renewed);

        // Still active at 7s (would have expired at 5s without renewal).
        let at_7s = now + Duration::from_secs(7);
        assert!(mgr.current_leader_at(ws, at_7s).is_some());
    }

    #[test]
    fn renew_not_found_for_unknown_workspace() {
        let mut mgr = LeaseManager::new();
        let (ws, client, _) = ids();
        let fake_lease = Uuid::new_v4();

        assert_eq!(mgr.renew(ws, client, fake_lease), RenewResult::NotFound);
    }

    #[test]
    fn renew_wrong_holder() {
        let mut mgr = LeaseManager::new();
        let (ws, client1, client2) = ids();

        let lease_id = match mgr.acquire(ws, client1) {
            AcquireResult::Granted { lease_id } => lease_id,
            _ => panic!("expected Granted"),
        };

        assert_eq!(mgr.renew(ws, client2, lease_id), RenewResult::WrongHolder);
    }

    #[test]
    fn renew_fails_after_expiry() {
        let mut mgr = LeaseManager::with_ttl(Duration::from_secs(1));
        let (ws, client, _) = ids();

        let now = Instant::now();
        let lease_id = match mgr.acquire_at(ws, client, now) {
            AcquireResult::Granted { lease_id } => lease_id,
            _ => panic!("expected Granted"),
        };

        let later = now + Duration::from_secs(2);
        assert_eq!(mgr.renew_at(ws, client, lease_id, later), RenewResult::NotFound);
    }

    // ── Release ────────────────────────────────────────────────────

    #[test]
    fn release_frees_lease() {
        let mut mgr = LeaseManager::new();
        let (ws, client, _) = ids();

        mgr.acquire(ws, client);
        assert_eq!(mgr.release(ws, client), ReleaseResult::Released);
        assert!(mgr.current_leader(ws).is_none());
    }

    #[test]
    fn release_wrong_holder() {
        let mut mgr = LeaseManager::new();
        let (ws, client1, client2) = ids();

        mgr.acquire(ws, client1);
        assert_eq!(mgr.release(ws, client2), ReleaseResult::WrongHolder);
    }

    #[test]
    fn release_not_found() {
        let mut mgr = LeaseManager::new();
        let (ws, client, _) = ids();

        assert_eq!(mgr.release(ws, client), ReleaseResult::NotFound);
    }

    #[test]
    fn release_not_found_after_expiry() {
        let mut mgr = LeaseManager::with_ttl(Duration::from_secs(1));
        let (ws, client, _) = ids();

        let now = Instant::now();
        mgr.acquire_at(ws, client, now);

        let later = now + Duration::from_secs(2);
        assert_eq!(mgr.release_at(ws, client, later), ReleaseResult::NotFound);
    }

    // ── Current leader ─────────────────────────────────────────────

    #[test]
    fn current_leader_returns_holder() {
        let mut mgr = LeaseManager::new();
        let (ws, client, _) = ids();

        mgr.acquire(ws, client);
        assert_eq!(mgr.current_leader(ws), Some(client));
    }

    #[test]
    fn current_leader_none_when_no_lease() {
        let mgr = LeaseManager::new();
        assert!(mgr.current_leader(Uuid::new_v4()).is_none());
    }

    #[test]
    fn current_leader_none_after_expiry() {
        let mut mgr = LeaseManager::with_ttl(Duration::from_secs(1));
        let (ws, client, _) = ids();

        let now = Instant::now();
        mgr.acquire_at(ws, client, now);

        let later = now + Duration::from_secs(2);
        assert!(mgr.current_leader_at(ws, later).is_none());
    }

    // ── Eviction ───────────────────────────────────────────────────

    #[test]
    fn evict_removes_expired_leases() {
        let mut mgr = LeaseManager::with_ttl(Duration::from_secs(1));
        let ws1 = Uuid::new_v4();
        let ws2 = Uuid::new_v4();
        let client = Uuid::new_v4();

        let now = Instant::now();
        mgr.acquire_at(ws1, client, now);
        mgr.acquire_at(ws2, client, now);

        assert_eq!(mgr.active_count_at(now), 2);

        let later = now + Duration::from_secs(2);
        let evicted = mgr.evict_expired_at(later);
        assert_eq!(evicted, 2);
        assert_eq!(mgr.active_count_at(later), 0);
    }

    #[test]
    fn evict_preserves_active_leases() {
        let mut mgr = LeaseManager::with_ttl(Duration::from_secs(10));
        let ws1 = Uuid::new_v4();
        let ws2 = Uuid::new_v4();
        let client = Uuid::new_v4();

        let now = Instant::now();
        mgr.acquire_at(ws1, client, now);
        mgr.acquire_at(ws2, client, now + Duration::from_secs(5));

        // At 8s: ws1 lease still active (expires at 10s), ws2 active (expires at 15s).
        let at_8s = now + Duration::from_secs(8);
        let evicted = mgr.evict_expired_at(at_8s);
        assert_eq!(evicted, 0);

        // At 12s: ws1 expired, ws2 still active.
        let at_12s = now + Duration::from_secs(12);
        let evicted = mgr.evict_expired_at(at_12s);
        assert_eq!(evicted, 1);
        assert_eq!(mgr.active_count_at(at_12s), 1);
    }

    // ── Multi-workspace ────────────────────────────────────────────

    #[test]
    fn different_workspaces_independent() {
        let mut mgr = LeaseManager::new();
        let ws1 = Uuid::new_v4();
        let ws2 = Uuid::new_v4();
        let client1 = Uuid::new_v4();
        let client2 = Uuid::new_v4();

        assert!(matches!(mgr.acquire(ws1, client1), AcquireResult::Granted { .. }));
        assert!(matches!(mgr.acquire(ws2, client2), AcquireResult::Granted { .. }));

        assert_eq!(mgr.current_leader(ws1), Some(client1));
        assert_eq!(mgr.current_leader(ws2), Some(client2));
    }

    // ── TTL configuration ──────────────────────────────────────────

    #[test]
    fn custom_ttl_respected() {
        let mut mgr = LeaseManager::with_ttl(Duration::from_millis(500));
        let (ws, client, _) = ids();

        let now = Instant::now();
        mgr.acquire_at(ws, client, now);

        // Active at 400ms.
        assert!(mgr.current_leader_at(ws, now + Duration::from_millis(400)).is_some());
        // Expired at 600ms.
        assert!(mgr.current_leader_at(ws, now + Duration::from_millis(600)).is_none());
    }

    #[test]
    fn default_ttl_is_60s() {
        let mgr = LeaseManager::new();
        assert_eq!(mgr.ttl, Duration::from_secs(60));
    }

    // ── After release, new client can acquire ──────────────────────

    #[test]
    fn new_client_acquires_after_release() {
        let mut mgr = LeaseManager::new();
        let (ws, client1, client2) = ids();

        mgr.acquire(ws, client1);
        mgr.release(ws, client1);

        let result = mgr.acquire(ws, client2);
        assert!(matches!(result, AcquireResult::Granted { .. }));
        assert_eq!(mgr.current_leader(ws), Some(client2));
    }
}
