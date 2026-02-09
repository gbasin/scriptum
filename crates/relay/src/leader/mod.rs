// Git leader-election lease coordination.
//
// One daemon per workspace holds the git-push lease at a time.
// Lease state is persisted in Postgres so leadership survives relay restarts
// and can coordinate safely across relay replicas.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
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
    pub granted_at: DateTime<Utc>,
    /// When the lease expires (unless renewed).
    pub expires_at: DateTime<Utc>,
}

impl Lease {
    /// Whether this lease has expired.
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(Utc::now())
    }

    fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }

    /// Remaining time on the lease.
    pub fn remaining(&self) -> Duration {
        if self.is_expired() {
            return Duration::ZERO;
        }

        (self.expires_at - Utc::now()).to_std().unwrap_or(Duration::ZERO)
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

/// Manages git leader-election leases for workspaces using Postgres.
#[derive(Clone)]
pub struct LeaseManager {
    pool: PgPool,
    ttl: Duration,
}

impl LeaseManager {
    pub fn new(pool: PgPool) -> Self {
        Self::with_ttl(pool, DEFAULT_LEASE_TTL)
    }

    pub fn with_ttl(pool: PgPool, ttl: Duration) -> Self {
        Self { pool, ttl }
    }

    /// Try to acquire the git leader lease for a workspace.
    ///
    /// - If no active lease exists, grants a new one.
    /// - If the caller already holds the lease, renews it.
    /// - If another daemon holds an active lease, denies.
    pub async fn acquire(
        &self,
        workspace_id: Uuid,
        client_id: Uuid,
    ) -> Result<AcquireResult, sqlx::Error> {
        let now = Utc::now();
        let expires_at = expires_at_with_ttl(now, self.ttl);
        let proposed_lease_id = Uuid::new_v4();

        let (holder_id, lease_id) = sqlx::query_as::<_, (Uuid, Uuid)>(
            r#"
INSERT INTO git_leader_leases (workspace_id, daemon_id, lease_id, acquired_at, expires_at)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (workspace_id) DO UPDATE
SET daemon_id = CASE
        WHEN git_leader_leases.expires_at <= $4 THEN EXCLUDED.daemon_id
        ELSE git_leader_leases.daemon_id
    END,
    lease_id = CASE
        WHEN git_leader_leases.expires_at <= $4 THEN EXCLUDED.lease_id
        ELSE git_leader_leases.lease_id
    END,
    acquired_at = CASE
        WHEN git_leader_leases.expires_at <= $4 THEN EXCLUDED.acquired_at
        ELSE git_leader_leases.acquired_at
    END,
    expires_at = CASE
        WHEN git_leader_leases.expires_at <= $4
            OR git_leader_leases.daemon_id = EXCLUDED.daemon_id
            THEN EXCLUDED.expires_at
        ELSE git_leader_leases.expires_at
    END
RETURNING daemon_id, lease_id
            "#,
        )
        .bind(workspace_id)
        .bind(client_id)
        .bind(proposed_lease_id)
        .bind(now)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;

        if holder_id != client_id {
            return Ok(AcquireResult::Denied { current_holder: holder_id });
        }

        if lease_id == proposed_lease_id {
            Ok(AcquireResult::Granted { lease_id })
        } else {
            Ok(AcquireResult::Renewed { lease_id })
        }
    }

    /// Renew an existing lease (heartbeat).
    pub async fn renew(
        &self,
        workspace_id: Uuid,
        client_id: Uuid,
        lease_id: Uuid,
    ) -> Result<RenewResult, sqlx::Error> {
        let now = Utc::now();
        let expires_at = expires_at_with_ttl(now, self.ttl);

        let updated = sqlx::query(
            r#"
UPDATE git_leader_leases
SET expires_at = $4
WHERE workspace_id = $1
  AND daemon_id = $2
  AND lease_id = $3
  AND expires_at > $5
            "#,
        )
        .bind(workspace_id)
        .bind(client_id)
        .bind(lease_id)
        .bind(expires_at)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if updated == 1 {
            return Ok(RenewResult::Renewed);
        }

        let active_holder = sqlx::query_scalar::<_, Uuid>(
            r#"
SELECT daemon_id
FROM git_leader_leases
WHERE workspace_id = $1
  AND expires_at > $2
            "#,
        )
        .bind(workspace_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(match active_holder {
            Some(_) => RenewResult::WrongHolder,
            None => RenewResult::NotFound,
        })
    }

    /// Release a lease voluntarily (daemon shutting down or done pushing).
    pub async fn release(
        &self,
        workspace_id: Uuid,
        client_id: Uuid,
    ) -> Result<ReleaseResult, sqlx::Error> {
        let now = Utc::now();

        let deleted = sqlx::query(
            r#"
DELETE FROM git_leader_leases
WHERE workspace_id = $1
  AND daemon_id = $2
  AND expires_at > $3
            "#,
        )
        .bind(workspace_id)
        .bind(client_id)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if deleted == 1 {
            return Ok(ReleaseResult::Released);
        }

        let active_holder = sqlx::query_scalar::<_, Uuid>(
            r#"
SELECT daemon_id
FROM git_leader_leases
WHERE workspace_id = $1
  AND expires_at > $2
            "#,
        )
        .bind(workspace_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(match active_holder {
            Some(_) => ReleaseResult::WrongHolder,
            None => ReleaseResult::NotFound,
        })
    }

    /// Get the current leader for a workspace (if any active lease exists).
    pub async fn current_leader(&self, workspace_id: Uuid) -> Result<Option<Uuid>, sqlx::Error> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
SELECT daemon_id
FROM git_leader_leases
WHERE workspace_id = $1
  AND expires_at > $2
            "#,
        )
        .bind(workspace_id)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await
    }

    /// Return the active lease details for a workspace.
    pub async fn current_lease(&self, workspace_id: Uuid) -> Result<Option<Lease>, sqlx::Error> {
        let row = sqlx::query_as::<_, (Uuid, Uuid, Uuid, DateTime<Utc>, DateTime<Utc>)>(
            r#"
SELECT workspace_id, daemon_id, lease_id, acquired_at, expires_at
FROM git_leader_leases
WHERE workspace_id = $1
  AND expires_at > $2
            "#,
        )
        .bind(workspace_id)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(workspace_id, holder_id, lease_id, granted_at, expires_at)| Lease {
            workspace_id,
            holder_id,
            lease_id,
            granted_at,
            expires_at,
        }))
    }

    /// Evict all expired leases. Returns the count of evicted leases.
    pub async fn evict_expired(&self) -> Result<usize, sqlx::Error> {
        let deleted = sqlx::query(
            r#"
DELETE FROM git_leader_leases
WHERE expires_at <= $1
            "#,
        )
        .bind(Utc::now())
        .execute(&self.pool)
        .await?
        .rows_affected();

        Ok(deleted as usize)
    }

    /// Number of active (non-expired) leases.
    pub async fn active_count(&self) -> Result<usize, sqlx::Error> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
SELECT COUNT(*)
FROM git_leader_leases
WHERE expires_at > $1
            "#,
        )
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await?;

        Ok(count as usize)
    }
}

impl Default for LeaseManager {
    fn default() -> Self {
        panic!("LeaseManager::default is not supported; construct with LeaseManager::new(pool)")
    }
}

fn expires_at_with_ttl(now: DateTime<Utc>, ttl: Duration) -> DateTime<Utc> {
    now + chrono::Duration::from_std(ttl).expect("lease ttl should fit within chrono::Duration")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        migrations::run_migrations,
        pool::{create_pg_pool, PoolConfig},
    };

    async fn test_pool() -> Option<PgPool> {
        let Some(database_url) = std::env::var("SCRIPTUM_RELAY_TEST_DATABASE_URL").ok() else {
            eprintln!("skipping leader postgres test: set SCRIPTUM_RELAY_TEST_DATABASE_URL");
            return None;
        };

        let pool = create_pg_pool(&database_url, PoolConfig::from_env())
            .await
            .expect("test postgres pool should connect");
        run_migrations(&pool).await.expect("relay migrations should apply");

        Some(pool)
    }

    async fn manager_with_ttl(ttl: Duration) -> Option<LeaseManager> {
        let pool = test_pool().await?;
        Some(LeaseManager::with_ttl(pool, ttl))
    }

    #[tokio::test]
    async fn acquire_persists_leader_and_denies_other_holder() {
        let Some(manager) = manager_with_ttl(DEFAULT_LEASE_TTL).await else {
            return;
        };

        let workspace_id = Uuid::new_v4();
        let daemon_a = Uuid::new_v4();
        let daemon_b = Uuid::new_v4();

        let first =
            manager.acquire(workspace_id, daemon_a).await.expect("first acquire should succeed");
        let lease_id = match first {
            AcquireResult::Granted { lease_id } => lease_id,
            other => panic!("expected Granted, got {other:?}"),
        };

        let lease = manager
            .current_lease(workspace_id)
            .await
            .expect("current lease query should succeed")
            .expect("lease should exist after acquire");
        assert_eq!(lease.workspace_id, workspace_id);
        assert_eq!(lease.holder_id, daemon_a);
        assert_eq!(lease.lease_id, lease_id);

        let denied = manager
            .acquire(workspace_id, daemon_b)
            .await
            .expect("second acquire should return denial");
        assert_eq!(denied, AcquireResult::Denied { current_holder: daemon_a });
    }

    #[tokio::test]
    async fn acquire_by_same_holder_renews_existing_lease() {
        let Some(manager) = manager_with_ttl(DEFAULT_LEASE_TTL).await else {
            return;
        };

        let workspace_id = Uuid::new_v4();
        let daemon_id = Uuid::new_v4();

        let first_lease = match manager
            .acquire(workspace_id, daemon_id)
            .await
            .expect("initial acquire should succeed")
        {
            AcquireResult::Granted { lease_id } => lease_id,
            other => panic!("expected Granted, got {other:?}"),
        };

        let second =
            manager.acquire(workspace_id, daemon_id).await.expect("re-acquire should succeed");
        assert_eq!(second, AcquireResult::Renewed { lease_id: first_lease });
    }

    #[tokio::test]
    async fn expired_lease_can_be_reacquired_by_new_holder() {
        let Some(manager) = manager_with_ttl(DEFAULT_LEASE_TTL).await else {
            return;
        };

        let workspace_id = Uuid::new_v4();
        let daemon_a = Uuid::new_v4();
        let daemon_b = Uuid::new_v4();

        let first_lease = match manager
            .acquire(workspace_id, daemon_a)
            .await
            .expect("initial acquire should succeed")
        {
            AcquireResult::Granted { lease_id } => lease_id,
            other => panic!("expected Granted, got {other:?}"),
        };

        sqlx::query(
            r#"
UPDATE git_leader_leases
SET expires_at = now() - interval '1 second'
WHERE workspace_id = $1
            "#,
        )
        .bind(workspace_id)
        .execute(&manager.pool)
        .await
        .expect("forced expiry update should succeed");

        let second_lease = match manager
            .acquire(workspace_id, daemon_b)
            .await
            .expect("acquire after expiry should succeed")
        {
            AcquireResult::Granted { lease_id } => lease_id,
            other => panic!("expected Granted after expiry, got {other:?}"),
        };

        assert_ne!(first_lease, second_lease);
        assert_eq!(
            manager
                .current_leader(workspace_id)
                .await
                .expect("current leader query should succeed"),
            Some(daemon_b)
        );
    }

    #[tokio::test]
    async fn renew_and_release_enforce_holder_identity() {
        let Some(manager) = manager_with_ttl(DEFAULT_LEASE_TTL).await else {
            return;
        };

        let workspace_id = Uuid::new_v4();
        let daemon_a = Uuid::new_v4();
        let daemon_b = Uuid::new_v4();

        let lease_id = match manager
            .acquire(workspace_id, daemon_a)
            .await
            .expect("initial acquire should succeed")
        {
            AcquireResult::Granted { lease_id } => lease_id,
            other => panic!("expected Granted, got {other:?}"),
        };

        assert_eq!(
            manager.renew(workspace_id, daemon_a, lease_id).await.expect("renew should succeed"),
            RenewResult::Renewed
        );
        assert_eq!(
            manager
                .renew(workspace_id, daemon_b, lease_id)
                .await
                .expect("renew by different holder should not fail query"),
            RenewResult::WrongHolder
        );
        assert_eq!(
            manager
                .renew(workspace_id, daemon_a, Uuid::new_v4())
                .await
                .expect("renew with stale lease id should not fail query"),
            RenewResult::WrongHolder
        );

        assert_eq!(
            manager
                .release(workspace_id, daemon_b)
                .await
                .expect("release by different holder should not fail query"),
            ReleaseResult::WrongHolder
        );
        assert_eq!(
            manager.release(workspace_id, daemon_a).await.expect("release should succeed"),
            ReleaseResult::Released
        );
        assert_eq!(
            manager
                .release(workspace_id, daemon_a)
                .await
                .expect("double release should not fail query"),
            ReleaseResult::NotFound
        );
    }

    #[tokio::test]
    async fn lease_state_is_visible_across_manager_instances() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let manager_a = LeaseManager::new(pool.clone());
        let manager_b = LeaseManager::new(pool);

        let workspace_id = Uuid::new_v4();
        let daemon_id = Uuid::new_v4();

        let acquire =
            manager_a.acquire(workspace_id, daemon_id).await.expect("acquire should succeed");
        assert!(matches!(acquire, AcquireResult::Granted { .. }));

        assert_eq!(
            manager_b
                .current_leader(workspace_id)
                .await
                .expect("cross-instance current leader query should succeed"),
            Some(daemon_id)
        );
        assert_eq!(manager_b.active_count().await.expect("active count query should succeed"), 1);
    }

    #[tokio::test]
    async fn evict_expired_removes_only_expired_rows() {
        let Some(manager) = manager_with_ttl(DEFAULT_LEASE_TTL).await else {
            return;
        };

        let workspace_expired = Uuid::new_v4();
        let workspace_active = Uuid::new_v4();
        let daemon_a = Uuid::new_v4();
        let daemon_b = Uuid::new_v4();

        manager.acquire(workspace_expired, daemon_a).await.expect("first acquire should succeed");
        manager.acquire(workspace_active, daemon_b).await.expect("second acquire should succeed");

        sqlx::query(
            r#"
UPDATE git_leader_leases
SET expires_at = now() - interval '1 second'
WHERE workspace_id = $1
            "#,
        )
        .bind(workspace_expired)
        .execute(&manager.pool)
        .await
        .expect("forced expiry update should succeed");

        let evicted = manager.evict_expired().await.expect("evict query should succeed");
        assert_eq!(evicted, 1);
        assert_eq!(manager.active_count().await.expect("active count query should succeed"), 1);
        assert_eq!(
            manager
                .current_leader(workspace_expired)
                .await
                .expect("expired leader query should succeed"),
            None
        );
        assert_eq!(
            manager
                .current_leader(workspace_active)
                .await
                .expect("active leader query should succeed"),
            Some(daemon_b)
        );
    }
}
