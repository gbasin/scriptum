#[path = "../src/db/pool.rs"]
mod pool;

use pool::{check_pool_health, create_pg_pool, PoolConfig};

#[tokio::test]
async fn pg_pool_connects_and_passes_health_check() {
    let Some(database_url) = std::env::var("SCRIPTUM_RELAY_TEST_DATABASE_URL").ok() else {
        eprintln!(
            "skipping db pool integration test: set SCRIPTUM_RELAY_TEST_DATABASE_URL to run it"
        );
        return;
    };

    let mut config = PoolConfig::default();
    config.min_connections = 1;
    config.max_connections = 2;

    let pool = create_pg_pool(&database_url, config)
        .await
        .expect("pool should connect to test database");

    check_pool_health(&pool)
        .await
        .expect("health check query should succeed");
}
