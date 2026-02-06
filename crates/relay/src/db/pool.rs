use std::env;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

const DEFAULT_MIN_CONNECTIONS: u32 = 2;
const DEFAULT_MAX_CONNECTIONS: u32 = 20;
const DEFAULT_ACQUIRE_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub min_connections: u32,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_connections: DEFAULT_MIN_CONNECTIONS,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            acquire_timeout: Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS),
        }
    }
}

impl PoolConfig {
    pub fn from_env() -> Self {
        let min_connections = env::var("SCRIPTUM_RELAY_DB_MIN_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(DEFAULT_MIN_CONNECTIONS);

        let max_connections = env::var("SCRIPTUM_RELAY_DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(DEFAULT_MAX_CONNECTIONS);

        let acquire_timeout_secs = env::var("SCRIPTUM_RELAY_DB_ACQUIRE_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_ACQUIRE_TIMEOUT_SECS);

        Self {
            min_connections,
            max_connections,
            acquire_timeout: Duration::from_secs(acquire_timeout_secs),
        }
    }
}

pub async fn create_pg_pool(database_url: &str, config: PoolConfig) -> Result<PgPool> {
    let connect_options = database_url
        .parse::<PgConnectOptions>()
        .context("failed to parse relay PostgreSQL connection options")?;

    PgPoolOptions::new()
        .min_connections(config.min_connections)
        .max_connections(config.max_connections)
        .acquire_timeout(config.acquire_timeout)
        .connect_with(connect_options)
        .await
        .context("failed to connect to relay PostgreSQL")
}

pub async fn check_pool_health(pool: &PgPool) -> Result<()> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .context("relay PostgreSQL health check failed")?;

    Ok(())
}
