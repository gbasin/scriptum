// File content hashing for change detection.
//
// Computes SHA-256 of file content and compares against the stored hash
// in meta.db to skip no-op saves (content unchanged despite mtime bump).

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

/// Hex-encoded SHA-256 hash of file content.
pub type ContentHash = String;

/// Compute the SHA-256 hash of the given bytes, returned as a lowercase hex string.
pub fn sha256_hex(content: &[u8]) -> ContentHash {
    let digest = Sha256::digest(content);
    hex_encode(&digest)
}

/// Compute the SHA-256 hash of a file on disk.
pub fn hash_file(path: &Path) -> Result<ContentHash> {
    let content = std::fs::read(path)
        .with_context(|| format!("failed to read file for hashing: {}", path.display()))?;
    Ok(sha256_hex(&content))
}

/// Look up the stored content hash for a document.
///
/// Returns `None` if the document is not tracked yet.
pub fn get_stored_hash(conn: &Connection, doc_id: &str) -> Result<Option<ContentHash>> {
    conn.query_row(
        "SELECT last_content_hash FROM documents_local WHERE doc_id = ?1",
        params![doc_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .context("failed to query stored content hash")
}

/// Update the stored content hash for a document.
///
/// Returns `true` if a row was updated (document exists in the table).
pub fn update_stored_hash(conn: &Connection, doc_id: &str, hash: &str) -> Result<bool> {
    let rows = conn
        .execute(
            "UPDATE documents_local SET last_content_hash = ?1 WHERE doc_id = ?2",
            params![hash, doc_id],
        )
        .context("failed to update stored content hash")?;
    Ok(rows > 0)
}

/// Check whether a file's content has changed compared to its stored hash.
///
/// Returns:
/// - `Ok(Some(new_hash))` if the content changed (or no stored hash exists).
/// - `Ok(None)` if the content is identical to the stored hash (no-op save).
pub fn check_content_changed(
    conn: &Connection,
    doc_id: &str,
    content: &[u8],
) -> Result<Option<ContentHash>> {
    let new_hash = sha256_hex(content);
    let stored = get_stored_hash(conn, doc_id)?;

    match stored {
        Some(old_hash) if old_hash == new_hash => Ok(None),
        _ => Ok(Some(new_hash)),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::params;
    use tempfile::TempDir;

    use super::*;
    use crate::store::meta_db::MetaDb;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_db_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("scriptum-test-{prefix}-{nanos}-{seq}"));
        std::fs::create_dir_all(&dir).expect("should create temp test dir");
        dir.join("meta.db")
    }

    fn cleanup_sqlite_files(path: &PathBuf) {
        let path_str = path.display().to_string();
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{path_str}-wal"));
        let _ = std::fs::remove_file(format!("{path_str}-shm"));
    }

    fn insert_doc(conn: &Connection, doc_id: &str, hash: &str) {
        conn.execute(
            "INSERT INTO documents_local (doc_id, workspace_id, abs_path, line_ending_style, \
             last_fs_mtime_ns, last_content_hash, projection_rev) \
             VALUES (?1, 'ws-1', '/test/doc.md', 'lf', 0, ?2, 0)",
            params![doc_id, hash],
        )
        .expect("insert should succeed");
    }

    // ── sha256_hex ─────────────────────────────────────────────────

    #[test]
    fn sha256_hex_empty() {
        // SHA-256 of empty input is the well-known constant.
        let hash = sha256_hex(b"");
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn sha256_hex_hello() {
        let hash = sha256_hex(b"hello");
        assert_eq!(hash, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[test]
    fn sha256_hex_deterministic() {
        let a = sha256_hex(b"# Hello World\n");
        let b = sha256_hex(b"# Hello World\n");
        assert_eq!(a, b);
    }

    #[test]
    fn sha256_hex_different_content_different_hash() {
        let a = sha256_hex(b"version 1");
        let b = sha256_hex(b"version 2");
        assert_ne!(a, b);
    }

    // ── hash_file ──────────────────────────────────────────────────

    #[test]
    fn hash_file_reads_and_hashes() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.md");
        std::fs::write(&path, "# Test\n").unwrap();

        let hash = hash_file(&path).unwrap();
        let expected = sha256_hex(b"# Test\n");
        assert_eq!(hash, expected);
    }

    #[test]
    fn hash_file_nonexistent_returns_error() {
        let result = hash_file(Path::new("/nonexistent/path/abc.md"));
        assert!(result.is_err());
    }

    // ── get_stored_hash / update_stored_hash ───────────────────────

    #[test]
    fn get_stored_hash_returns_none_for_unknown_doc() {
        let db_path = unique_temp_db_path("hash-get-none");
        let db = MetaDb::open(&db_path).unwrap();

        let result = get_stored_hash(db.connection(), "nonexistent").unwrap();
        assert!(result.is_none());

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    #[test]
    fn get_stored_hash_returns_existing_hash() {
        let db_path = unique_temp_db_path("hash-get-existing");
        let db = MetaDb::open(&db_path).unwrap();

        insert_doc(db.connection(), "doc-1", "abc123");
        let result = get_stored_hash(db.connection(), "doc-1").unwrap();
        assert_eq!(result, Some("abc123".to_string()));

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    #[test]
    fn update_stored_hash_updates_existing_doc() {
        let db_path = unique_temp_db_path("hash-update");
        let db = MetaDb::open(&db_path).unwrap();

        insert_doc(db.connection(), "doc-1", "old-hash");
        let updated = update_stored_hash(db.connection(), "doc-1", "new-hash").unwrap();
        assert!(updated);

        let current = get_stored_hash(db.connection(), "doc-1").unwrap();
        assert_eq!(current, Some("new-hash".to_string()));

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    #[test]
    fn update_stored_hash_returns_false_for_unknown_doc() {
        let db_path = unique_temp_db_path("hash-update-missing");
        let db = MetaDb::open(&db_path).unwrap();

        let updated = update_stored_hash(db.connection(), "nonexistent", "hash").unwrap();
        assert!(!updated);

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    // ── check_content_changed ──────────────────────────────────────

    #[test]
    fn check_content_changed_returns_hash_when_new_doc() {
        let db_path = unique_temp_db_path("hash-check-new");
        let db = MetaDb::open(&db_path).unwrap();

        let result = check_content_changed(db.connection(), "new-doc", b"content").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), sha256_hex(b"content"));

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    #[test]
    fn check_content_changed_returns_none_when_unchanged() {
        let db_path = unique_temp_db_path("hash-check-same");
        let db = MetaDb::open(&db_path).unwrap();

        let hash = sha256_hex(b"# Hello\n");
        insert_doc(db.connection(), "doc-1", &hash);

        let result = check_content_changed(db.connection(), "doc-1", b"# Hello\n").unwrap();
        assert!(result.is_none());

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    #[test]
    fn check_content_changed_returns_hash_when_different() {
        let db_path = unique_temp_db_path("hash-check-diff");
        let db = MetaDb::open(&db_path).unwrap();

        let old_hash = sha256_hex(b"version 1");
        insert_doc(db.connection(), "doc-1", &old_hash);

        let result = check_content_changed(db.connection(), "doc-1", b"version 2").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), sha256_hex(b"version 2"));

        drop(db);
        cleanup_sqlite_files(&db_path);
    }

    // ── hex_encode ─────────────────────────────────────────────────

    #[test]
    fn hex_encode_produces_lowercase() {
        let hash = sha256_hex(b"test");
        assert_eq!(hash, hash.to_lowercase());
    }

    #[test]
    fn hex_encode_length_is_64() {
        let hash = sha256_hex(b"anything");
        assert_eq!(hash.len(), 64);
    }
}
