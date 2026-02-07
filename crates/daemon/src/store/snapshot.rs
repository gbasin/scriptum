use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::security::{
    decrypt_at_rest, encrypt_at_rest, ensure_owner_only_dir, ensure_owner_only_file,
    open_private_truncate,
};

const SNAPSHOT_FILE_EXT: &str = "snap";
const SNAPSHOT_MAGIC: [u8; 4] = *b"SNP1";
const SNAPSHOT_VERSION: u8 = 1;
const SNAPSHOT_HEADER_BYTES: usize = 18;

pub const SNAPSHOT_INTERVAL_UPDATES: i64 = 1_000;
pub const SNAPSHOT_INTERVAL_MINUTES: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotCodec {
    Raw = 0,
    Rle = 1,
}

impl SnapshotCodec {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Raw),
            1 => Some(Self::Rle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub snapshot_seq: i64,
    pub payload: Vec<u8>,
    pub codec: SnapshotCodec,
}

/// Stores full-document snapshots at `crdt_store/snapshots/{doc_id}.snap`.
#[derive(Debug, Clone)]
pub struct SnapshotStore {
    snapshots_dir: PathBuf,
}

impl SnapshotStore {
    pub fn new(crdt_store_dir: impl AsRef<Path>) -> Result<Self> {
        let snapshots_dir = crdt_store_dir.as_ref().join("snapshots");
        fs::create_dir_all(&snapshots_dir).with_context(|| {
            format!("failed to create snapshots directory `{}`", snapshots_dir.display())
        })?;
        ensure_owner_only_dir(&snapshots_dir)?;
        Ok(Self { snapshots_dir })
    }

    pub fn save_snapshot(
        &self,
        doc_id: Uuid,
        snapshot_seq: i64,
        payload: &[u8],
    ) -> Result<PathBuf> {
        let (codec, encoded_payload) = encode_payload(payload);
        let encrypted_payload =
            encrypt_at_rest(&encoded_payload).context("failed to encrypt snapshot at rest")?;
        let mut header = [0u8; SNAPSHOT_HEADER_BYTES];
        header[..4].copy_from_slice(&SNAPSHOT_MAGIC);
        header[4] = SNAPSHOT_VERSION;
        header[5] = codec as u8;
        header[6..14].copy_from_slice(&snapshot_seq.to_le_bytes());
        header[14..18].copy_from_slice(
            &u32::try_from(payload.len())
                .context("snapshot payload exceeds u32::MAX")?
                .to_le_bytes(),
        );

        let target_path = self.snapshot_path(doc_id);
        let tmp_path = self.temp_path_for(doc_id);
        let mut file = open_private_truncate(&tmp_path).with_context(|| {
            format!("failed to open temp snapshot `{}`", tmp_path.display())
        })?;
        ensure_owner_only_file(&tmp_path)?;

        file.write_all(&header).context("failed to write snapshot header")?;
        file.write_all(&encrypted_payload).context("failed to write snapshot payload")?;
        file.sync_data().context("failed to fsync snapshot file")?;
        drop(file);

        fs::rename(&tmp_path, &target_path).with_context(|| {
            format!(
                "failed to atomically move snapshot `{}` to `{}`",
                tmp_path.display(),
                target_path.display()
            )
        })?;
        ensure_owner_only_file(&target_path)?;

        Ok(target_path)
    }

    pub fn load_snapshot(&self, doc_id: Uuid) -> Result<Option<SnapshotRecord>> {
        let path = self.snapshot_path(doc_id);
        if !path.exists() {
            return Ok(None);
        }

        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .with_context(|| format!("failed to open snapshot `{}`", path.display()))?;

        let mut header = [0u8; SNAPSHOT_HEADER_BYTES];
        file.read_exact(&mut header)
            .with_context(|| format!("snapshot `{}` has truncated header", path.display()))?;

        if header[..4] != SNAPSHOT_MAGIC {
            bail!("snapshot `{}` has invalid magic", path.display());
        }

        if header[4] != SNAPSHOT_VERSION {
            bail!("snapshot `{}` has unsupported version {}", path.display(), header[4]);
        }

        let codec = SnapshotCodec::from_u8(header[5]).with_context(|| {
            format!("snapshot `{}` has unknown codec {}", path.display(), header[5])
        })?;
        let snapshot_seq = i64::from_le_bytes(header[6..14].try_into().expect("seq header slice"));
        let expected_len =
            u32::from_le_bytes(header[14..18].try_into().expect("length header slice")) as usize;

        let mut encoded_payload = Vec::new();
        file.read_to_end(&mut encoded_payload).context("failed to read snapshot payload")?;
        let encoded_payload = decrypt_at_rest(&encoded_payload)
            .context("failed to decrypt snapshot payload at rest")?;
        let payload = decode_payload(codec, &encoded_payload, expected_len)?;

        Ok(Some(SnapshotRecord { snapshot_seq, payload, codec }))
    }

    pub fn should_snapshot(
        &self,
        last_snapshot_seq: i64,
        current_seq: i64,
        last_snapshot_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> bool {
        let updates_since_snapshot = current_seq.saturating_sub(last_snapshot_seq);
        updates_since_snapshot >= SNAPSHOT_INTERVAL_UPDATES
            || now.signed_duration_since(last_snapshot_at)
                >= Duration::minutes(SNAPSHOT_INTERVAL_MINUTES)
    }

    pub fn snapshot_path(&self, doc_id: Uuid) -> PathBuf {
        self.snapshots_dir.join(format!("{}.{}", doc_id, SNAPSHOT_FILE_EXT))
    }

    fn temp_path_for(&self, doc_id: Uuid) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        self.snapshots_dir.join(format!("{}.tmp.{}", doc_id, nonce))
    }
}

fn encode_payload(payload: &[u8]) -> (SnapshotCodec, Vec<u8>) {
    let rle = rle_compress(payload);
    if rle.len() < payload.len() {
        (SnapshotCodec::Rle, rle)
    } else {
        (SnapshotCodec::Raw, payload.to_vec())
    }
}

fn decode_payload(
    codec: SnapshotCodec,
    encoded_payload: &[u8],
    expected_len: usize,
) -> Result<Vec<u8>> {
    let decoded = match codec {
        SnapshotCodec::Raw => encoded_payload.to_vec(),
        SnapshotCodec::Rle => rle_decompress(encoded_payload, expected_len)?,
    };

    if decoded.len() != expected_len {
        bail!(
            "decoded snapshot payload length mismatch: expected {}, got {}",
            expected_len,
            decoded.len()
        );
    }

    Ok(decoded)
}

fn rle_compress(input: &[u8]) -> Vec<u8> {
    if input.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(input.len());
    let mut run_byte = input[0];
    let mut run_len = 1u8;

    for &byte in &input[1..] {
        if byte == run_byte && run_len < u8::MAX {
            run_len = run_len.saturating_add(1);
            continue;
        }

        out.push(run_len);
        out.push(run_byte);
        run_byte = byte;
        run_len = 1;
    }

    out.push(run_len);
    out.push(run_byte);
    out
}

fn rle_decompress(input: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    if !input.len().is_multiple_of(2) {
        bail!("invalid rle payload length {}", input.len());
    }

    let mut out = Vec::with_capacity(expected_len);
    for pair in input.chunks_exact(2) {
        let run_len = usize::from(pair[0]);
        let run_byte = pair[1];
        out.extend(std::iter::repeat_n(run_byte, run_len));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{SnapshotCodec, SnapshotStore};
    use crate::engine::ydoc::YDoc;
    use chrono::{Duration, Utc};
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn creates_and_loads_snapshot_round_trip() {
        let tmp = tempdir().expect("tempdir should be created");
        let store = SnapshotStore::new(tmp.path().join("crdt_store")).expect("snapshot store");
        let doc_id = Uuid::new_v4();

        let doc = YDoc::with_client_id(7);
        doc.insert_text("content", 0, "persisted snapshot state");
        let encoded_state = doc.encode_state();

        store.save_snapshot(doc_id, 42, &encoded_state).expect("snapshot should be saved");

        let loaded = store
            .load_snapshot(doc_id)
            .expect("snapshot should load")
            .expect("snapshot should exist");
        assert_eq!(loaded.snapshot_seq, 42);

        let restored_doc =
            YDoc::from_state(&loaded.payload).expect("state should restore into YDoc");
        assert_eq!(restored_doc.get_text_string("content"), "persisted snapshot state");
    }

    #[test]
    fn compresses_repetitive_snapshot_payload() {
        let tmp = tempdir().expect("tempdir should be created");
        let store = SnapshotStore::new(tmp.path().join("crdt_store")).expect("snapshot store");
        let doc_id = Uuid::new_v4();
        let payload = vec![b'x'; 8192];

        let path = store.save_snapshot(doc_id, 1, &payload).expect("snapshot should be saved");
        let loaded = store
            .load_snapshot(doc_id)
            .expect("snapshot should load")
            .expect("snapshot should exist");

        assert_eq!(loaded.codec, SnapshotCodec::Rle);
        assert_eq!(loaded.payload, payload);

        let file_len =
            std::fs::metadata(path).expect("snapshot metadata should be readable").len() as usize;
        assert!(file_len < payload.len());
    }

    #[test]
    fn snapshot_policy_uses_sequence_or_time_threshold() {
        let tmp = tempdir().expect("tempdir should be created");
        let store = SnapshotStore::new(tmp.path().join("crdt_store")).expect("snapshot store");
        let now = Utc::now();

        assert!(store.should_snapshot(0, 1000, now, now));
        assert!(store.should_snapshot(10, 100, now - Duration::minutes(10), now));
        assert!(!store.should_snapshot(10, 999, now - Duration::minutes(9), now));
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempdir().expect("tempdir should be created");
        let store = SnapshotStore::new(tmp.path().join("crdt_store")).expect("snapshot store");
        let doc_id = Uuid::new_v4();

        let path = store
            .save_snapshot(doc_id, 1, b"snapshot payload")
            .expect("snapshot should be saved");

        let mode = std::fs::metadata(path)
            .expect("snapshot metadata should load")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
