#[path = "../src/db/migrations.rs"]
mod migrations;
#[path = "../src/db/pool.rs"]
mod pool;

use pool::{create_pg_pool, PoolConfig};

const EXPECTED_TABLES: &[&str] = &[
    "users",
    "refresh_sessions",
    "workspaces",
    "workspace_members",
    "documents",
    "tags",
    "document_tags",
    "backlinks",
    "comment_threads",
    "comment_messages",
    "share_links",
    "acl_overrides",
    "yjs_update_log",
    "yjs_snapshots",
    "idempotency_keys",
    "audit_events",
];

#[tokio::test]
async fn relay_migrations_create_expected_tables() {
    let Some(database_url) = std::env::var("SCRIPTUM_RELAY_TEST_DATABASE_URL").ok() else {
        eprintln!("skipping db migration integration test: set SCRIPTUM_RELAY_TEST_DATABASE_URL");
        return;
    };

    let config = PoolConfig { min_connections: 1, max_connections: 2, ..PoolConfig::default() };

    let pool =
        create_pg_pool(&database_url, config).await.expect("pool should connect to test database");

    migrations::run_migrations(&pool).await.expect("migrations should apply");

    let table_names: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT table_name \
         FROM information_schema.tables \
         WHERE table_schema = 'public'",
    )
    .fetch_all(&pool)
    .await
    .expect("table lookup should succeed");

    for expected_table in EXPECTED_TABLES {
        assert!(
            table_names.iter().any(|name| name == expected_table),
            "expected table `{expected_table}` to exist after migrations"
        );
    }
}
