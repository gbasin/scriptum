use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

const MIGRATION_V1_SQL: &str = r#"
CREATE TABLE documents_local (
    doc_id              TEXT PRIMARY KEY,
    workspace_id        TEXT NOT NULL,
    abs_path            TEXT NOT NULL,
    line_ending_style   TEXT NOT NULL,
    last_fs_mtime_ns    INTEGER NOT NULL,
    last_content_hash   TEXT NOT NULL,
    projection_rev      INTEGER NOT NULL,
    last_server_seq     INTEGER NOT NULL DEFAULT 0,
    last_ack_seq        INTEGER NOT NULL DEFAULT 0,
    parse_error         TEXT NULL
);

CREATE TABLE agent_sessions (
    session_id      TEXT PRIMARY KEY,
    agent_id        TEXT NOT NULL,
    workspace_id    TEXT NOT NULL,
    started_at      TEXT NOT NULL,
    last_seen_at    TEXT NOT NULL,
    status          TEXT NOT NULL
);

CREATE TABLE agent_recent_edits (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id              TEXT NOT NULL,
    agent_id            TEXT NOT NULL,
    start_offset_utf16  INTEGER NOT NULL,
    end_offset_utf16    INTEGER NOT NULL,
    ts                  TEXT NOT NULL
);

CREATE TABLE git_sync_config (
    workspace_id        TEXT PRIMARY KEY,
    mode                TEXT NOT NULL,
    remote_name         TEXT NOT NULL DEFAULT 'origin',
    branch              TEXT NOT NULL DEFAULT 'main',
    commit_interval_sec INTEGER NOT NULL DEFAULT 30,
    push_policy         TEXT NOT NULL DEFAULT 'disabled',
    ai_enabled          INTEGER NOT NULL DEFAULT 1,
    redaction_policy    TEXT NOT NULL DEFAULT 'redacted'
);

CREATE TABLE git_sync_jobs (
    job_id              TEXT PRIMARY KEY,
    workspace_id        TEXT NOT NULL,
    state               TEXT NOT NULL,
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    next_attempt_at     TEXT NULL,
    last_error_code     TEXT NULL,
    last_error_message  TEXT NULL,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE TABLE outbox_updates (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id        TEXT NOT NULL,
    doc_id              TEXT NOT NULL,
    client_update_id    TEXT NOT NULL,
    payload             BLOB NOT NULL,
    retry_count         INTEGER NOT NULL DEFAULT 0,
    next_retry_at       TEXT NULL,
    state               TEXT NOT NULL DEFAULT 'pending',
    created_at          TEXT NOT NULL
);
"#;

const MIGRATION_V2_SQL: &str = r#"
CREATE TABLE agent_leases (
    workspace_id    TEXT NOT NULL,
    doc_id          TEXT NOT NULL,
    section_id      TEXT NOT NULL,
    agent_id        TEXT NOT NULL,
    ttl_sec         INTEGER NOT NULL,
    mode            TEXT NOT NULL CHECK (mode IN ('exclusive', 'shared')),
    note            TEXT NULL,
    expires_at      TEXT NOT NULL,
    PRIMARY KEY (workspace_id, doc_id, section_id, agent_id)
);

CREATE INDEX agent_leases_expires_idx
    ON agent_leases (expires_at);

CREATE INDEX agent_leases_lookup_idx
    ON agent_leases (workspace_id, doc_id, section_id);
"#;

const MIGRATIONS: &[(i64, &str)] = &[(1, MIGRATION_V1_SQL), (2, MIGRATION_V2_SQL)];

#[derive(Debug)]
pub struct MetaDb {
    conn: Connection,
}

impl MetaDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create meta.db parent directory `{}`", parent.display())
            })?;
        }

        let mut conn = Connection::open(path)
            .with_context(|| format!("failed to open meta.db at `{}`", path.display()))?;

        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            ",
        )
        .context("failed to configure sqlite pragmas for meta.db")?;

        ensure_migration_table(&conn)?;
        apply_pending_migrations(&mut conn)?;

        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn schema_version(&self) -> Result<i64> {
        current_schema_version(&self.conn)
    }
}

fn ensure_migration_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  TEXT NOT NULL
        );
        ",
    )
    .context("failed to ensure schema_migrations table exists")
}

fn current_schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .context("failed to read current schema version")
}

fn apply_pending_migrations(conn: &mut Connection) -> Result<()> {
    let mut current_version = current_schema_version(conn)?;

    for (version, sql) in MIGRATIONS {
        if *version <= current_version {
            continue;
        }

        let tx = conn.transaction().context("failed to start migration transaction")?;
        tx.execute_batch(sql)
            .with_context(|| format!("failed to apply meta.db migration v{version}"))?;
        tx.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
            params![version],
        )
        .with_context(|| format!("failed to record migration v{version}"))?;
        tx.commit().with_context(|| format!("failed to commit migration v{version}"))?;
        current_version = *version;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use super::{MetaDb, MIGRATION_V1_SQL};

    const EXPECTED_TABLES: &[&str] = &[
        "schema_migrations",
        "documents_local",
        "agent_sessions",
        "agent_recent_edits",
        "agent_leases",
        "git_sync_config",
        "git_sync_jobs",
        "outbox_updates",
    ];

    #[test]
    fn open_creates_schema_and_records_latest_migration() {
        let db_path = unique_temp_db_path("meta-db-schema");
        let db = MetaDb::open(&db_path).expect("meta db should open");

        for table in EXPECTED_TABLES {
            let exists: i64 = db
                .connection()
                .query_row(
                    "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("table existence query should succeed");

            assert_eq!(exists, 1, "expected `{table}` table to exist");
        }

        assert_eq!(db.schema_version().expect("schema version should be readable"), 2);

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    #[test]
    fn opening_twice_is_idempotent_for_all_migrations() {
        let db_path = unique_temp_db_path("meta-db-idempotent");
        {
            let first = MetaDb::open(&db_path).expect("first open should succeed");
            assert_eq!(first.schema_version().expect("schema version should be readable"), 2);
        }

        let second = MetaDb::open(&db_path).expect("second open should succeed");
        let migration_rows: i64 = second
            .connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
            .expect("schema migration count query should succeed");
        assert_eq!(migration_rows, 2);

        drop(second);
        cleanup_sqlite_files(&db_path);
    }

    #[test]
    fn existing_v1_schema_is_migrated_to_v2() {
        let db_path = unique_temp_db_path("meta-db-upgrade-v1-v2");
        seed_v1_schema(&db_path);

        let db = MetaDb::open(&db_path).expect("meta db should upgrade from v1 to v2");
        assert_eq!(db.schema_version().expect("schema version should be readable"), 2);

        let lease_table_exists: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'agent_leases'",
                [],
                |row| row.get(0),
            )
            .expect("lease table existence query should succeed");
        assert_eq!(lease_table_exists, 1);

        let migration_rows: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
            .expect("schema migration count query should succeed");
        assert_eq!(migration_rows, 2);

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    fn unique_temp_db_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("scriptum-{prefix}-{nanos}.db"))
    }

    fn cleanup_sqlite_files(path: &PathBuf) {
        let path_str = path.display().to_string();
        let wal = format!("{path_str}-wal");
        let shm = format!("{path_str}-shm");

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(wal);
        let _ = std::fs::remove_file(shm);
    }

    fn seed_v1_schema(path: &PathBuf) {
        let conn = Connection::open(path).expect("v1 seed db should open");
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version     INTEGER PRIMARY KEY,
                applied_at  TEXT NOT NULL
            );
            ",
        )
        .expect("schema_migrations should be created");
        conn.execute_batch(MIGRATION_V1_SQL).expect("v1 schema should be applied");
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (1, datetime('now'))",
            [],
        )
        .expect("v1 migration row should be inserted");
    }
}
