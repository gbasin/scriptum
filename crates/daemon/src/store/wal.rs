use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::security::{
    decrypt_at_rest, encrypt_at_rest, ensure_owner_only_dir, ensure_owner_only_file,
    open_private_append,
};

const FRAME_HEADER_BYTES: usize = 8;
// 1 MiB payload cap plus envelope overhead for at-rest encryption metadata.
const MAX_UPDATE_BYTES: usize = (1 << 20) + 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalReplaySummary {
    pub applied: usize,
    pub valid_frames: usize,
    pub truncated: bool,
    pub checksum_failed: bool,
}

/// Minimal append-only WAL format:
/// [len:u32 little-endian][checksum:u32 little-endian][payload:len bytes]
#[derive(Debug, Clone)]
pub struct WalStore {
    path: PathBuf,
}

impl WalStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create wal directory `{}`", parent.display())
            })?;
            ensure_owner_only_dir(parent)?;
        }

        // Ensure the file exists so replay can open it consistently.
        open_private_append(&path)
            .with_context(|| format!("failed to open wal file `{}`", path.display()))?;
        ensure_owner_only_file(&path)?;

        Ok(Self { path })
    }

    pub fn for_doc(base_dir: impl AsRef<Path>, workspace_id: Uuid, doc_id: Uuid) -> Result<Self> {
        let wal_path =
            base_dir.as_ref().join(workspace_id.to_string()).join(format!("{doc_id}.wal"));
        Self::open(wal_path)
    }

    pub fn append_update(&self, payload: &[u8]) -> Result<()> {
        let encrypted_payload =
            encrypt_at_rest(payload).context("failed to encrypt wal payload at rest")?;
        let len = u32::try_from(encrypted_payload.len()).context("wal payload exceeds u32::MAX")?;
        let checksum = checksum(&encrypted_payload);
        let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + encrypted_payload.len());
        frame.extend_from_slice(&len.to_le_bytes());
        frame.extend_from_slice(&checksum.to_le_bytes());
        frame.extend_from_slice(&encrypted_payload);

        let mut file = open_private_append(&self.path).with_context(|| {
            format!("failed to open wal file `{}` for append", self.path.display())
        })?;
        ensure_owner_only_file(&self.path)?;

        file.write_all(&frame).context("failed to write wal frame payload")?;
        file.sync_data().context("failed to fsync wal file")?;
        Ok(())
    }

    /// Replay WAL updates in order. Corrupted/truncated frames are treated as a recoverable tail:
    /// replay stops at the first bad frame and the WAL is truncated to the last valid offset.
    pub fn replay<F>(&self, mut on_update: F) -> Result<usize>
    where
        F: FnMut(&[u8]) -> Result<()>,
    {
        Ok(self.replay_from_frame(0, |payload| on_update(payload))?.applied)
    }

    /// Replay WAL updates starting from `start_frame` (0-based), validating all frames.
    ///
    /// Frames before `start_frame` are validated and skipped. Corrupted/truncated frames are
    /// treated as a recoverable tail: replay stops at the first bad frame and the WAL is
    /// truncated to the last valid offset.
    pub fn replay_from_frame<F>(
        &self,
        start_frame: usize,
        mut on_update: F,
    ) -> Result<WalReplaySummary>
    where
        F: FnMut(&[u8]) -> Result<()>,
    {
        let mut file = OpenOptions::new().read(true).open(&self.path).with_context(|| {
            format!("failed to open wal file `{}` for replay", self.path.display())
        })?;

        let mut applied = 0usize;
        let mut valid_frames = 0usize;
        let mut truncate_to = None;
        let mut checksum_failed = false;
        loop {
            let frame_offset =
                file.stream_position().context("failed to read wal stream position")?;
            let mut header = [0u8; FRAME_HEADER_BYTES];
            let bytes_read = file.read(&mut header).context("failed reading wal frame header")?;
            if bytes_read == 0 {
                break;
            }

            if bytes_read < FRAME_HEADER_BYTES
                && file.read_exact(&mut header[bytes_read..]).is_err()
            {
                truncate_to = Some(frame_offset);
                break;
            }

            let len =
                u32::from_le_bytes(header[..4].try_into().expect("header length slice")) as usize;
            if len > MAX_UPDATE_BYTES {
                truncate_to = Some(frame_offset);
                break;
            }

            let expected_checksum =
                u32::from_le_bytes(header[4..].try_into().expect("header checksum slice"));
            let mut payload = vec![0u8; len];
            if file.read_exact(&mut payload).is_err() {
                truncate_to = Some(frame_offset);
                break;
            }

            let actual_checksum = checksum(&payload);
            if expected_checksum != actual_checksum {
                truncate_to = Some(frame_offset);
                checksum_failed = true;
                break;
            }

            let payload = match decrypt_at_rest(&payload) {
                Ok(payload) => payload,
                Err(_) => {
                    truncate_to = Some(frame_offset);
                    checksum_failed = true;
                    break;
                }
            };

            if valid_frames >= start_frame {
                on_update(&payload).context("failed to apply wal frame payload")?;
                applied = applied.saturating_add(1);
            }
            valid_frames = valid_frames.saturating_add(1);
        }

        drop(file);
        if let Some(offset) = truncate_to {
            truncate_wal(&self.path, offset)?;
        }

        Ok(WalReplaySummary {
            applied,
            valid_frames,
            truncated: truncate_to.is_some(),
            checksum_failed,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn checksum(payload: &[u8]) -> u32 {
    // FNV-1a 32-bit checksum for simple corruption detection.
    let mut hash = 0x811c9dc5u32;
    for byte in payload {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn truncate_wal(path: &Path, offset: u64) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open wal file `{}` for truncation", path.display()))?;
    file.set_len(offset)
        .with_context(|| format!("failed to truncate wal file `{}` to {offset}", path.display()))?;
    file.sync_data()
        .with_context(|| format!("failed to fsync truncated wal file `{}`", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::path::Path;

    use super::{WalReplaySummary, WalStore, FRAME_HEADER_BYTES};
    use tempfile::tempdir;
    use uuid::Uuid;

    fn first_frame_len(path: &Path) -> usize {
        let mut file =
            OpenOptions::new().read(true).open(path).expect("wal file should open for reads");
        let mut header = [0u8; FRAME_HEADER_BYTES];
        file.read_exact(&mut header).expect("first wal frame header should be readable");
        let payload_len =
            u32::from_le_bytes(header[..4].try_into().expect("frame length bytes")) as usize;
        FRAME_HEADER_BYTES + payload_len
    }

    #[test]
    fn append_and_replay_round_trip() {
        let tmp = tempdir().expect("tempdir should be created");
        let wal = WalStore::open(tmp.path().join("doc.wal")).expect("wal should open");

        wal.append_update(b"u1").expect("frame 1 should append");
        wal.append_update(b"u2").expect("frame 2 should append");

        let mut updates = Vec::new();
        let applied = wal
            .replay(|payload| {
                updates.push(payload.to_vec());
                Ok(())
            })
            .expect("wal replay should succeed");

        assert_eq!(applied, 2);
        assert_eq!(updates, vec![b"u1".to_vec(), b"u2".to_vec()]);
    }

    #[test]
    fn replay_truncates_corrupted_tail() {
        let tmp = tempdir().expect("tempdir should be created");
        let wal = WalStore::open(tmp.path().join("doc.wal")).expect("wal should open");

        wal.append_update(b"u1").expect("frame 1 should append");
        wal.append_update(b"u2").expect("frame 2 should append");

        let first_frame_len = first_frame_len(wal.path());
        let second_frame_checksum_offset = first_frame_len + 4;

        let mut file = OpenOptions::new()
            .write(true)
            .open(wal.path())
            .expect("wal file should open for corruption");
        file.seek(SeekFrom::Start(second_frame_checksum_offset as u64)).expect("seek should work");
        file.write_all(&[0, 0, 0, 0]).expect("checksum bytes should be overwritten");
        file.sync_data().expect("corruption write should fsync");

        let mut replayed = Vec::new();
        let applied = wal
            .replay(|payload| {
                replayed.push(payload.to_vec());
                Ok(())
            })
            .expect("replay should recover from corruption");

        assert_eq!(applied, 1);
        assert_eq!(replayed, vec![b"u1".to_vec()]);
        assert_eq!(
            std::fs::metadata(wal.path()).expect("wal metadata should be readable").len(),
            first_frame_len as u64
        );

        let mut replayed_again = Vec::new();
        let applied_again = wal
            .replay(|payload| {
                replayed_again.push(payload.to_vec());
                Ok(())
            })
            .expect("replay should be stable after truncation");
        assert_eq!(applied_again, 1);
        assert_eq!(replayed_again, vec![b"u1".to_vec()]);
    }

    #[test]
    fn replay_from_frame_skips_snapshot_covered_updates() {
        let tmp = tempdir().expect("tempdir should be created");
        let wal = WalStore::open(tmp.path().join("doc.wal")).expect("wal should open");

        wal.append_update(b"u1").expect("frame 1 should append");
        wal.append_update(b"u2").expect("frame 2 should append");
        wal.append_update(b"u3").expect("frame 3 should append");

        let mut replayed = Vec::new();
        let summary = wal
            .replay_from_frame(2, |payload| {
                replayed.push(payload.to_vec());
                Ok(())
            })
            .expect("replay should succeed");

        assert_eq!(
            summary,
            WalReplaySummary {
                applied: 1,
                valid_frames: 3,
                truncated: false,
                checksum_failed: false,
            }
        );
        assert_eq!(replayed, vec![b"u3".to_vec()]);
    }

    #[test]
    fn replay_summary_marks_checksum_failures() {
        let tmp = tempdir().expect("tempdir should be created");
        let wal = WalStore::open(tmp.path().join("doc.wal")).expect("wal should open");
        wal.append_update(b"u1").expect("frame 1 should append");
        wal.append_update(b"u2").expect("frame 2 should append");

        let first_frame_len = first_frame_len(wal.path());
        let second_frame_checksum_offset = first_frame_len + 4;

        let mut file = OpenOptions::new()
            .write(true)
            .open(wal.path())
            .expect("wal file should open for corruption");
        file.seek(SeekFrom::Start(second_frame_checksum_offset as u64)).expect("seek should work");
        file.write_all(&[0, 0, 0, 0]).expect("checksum bytes should be overwritten");
        file.sync_data().expect("corruption write should fsync");

        let summary = wal
            .replay_from_frame(0, |_payload| Ok(()))
            .expect("replay should recover from checksum failure");

        assert_eq!(
            summary,
            WalReplaySummary {
                applied: 1,
                valid_frames: 1,
                truncated: true,
                checksum_failed: true,
            }
        );
    }

    #[test]
    fn for_doc_uses_per_doc_wal_path() {
        let tmp = tempdir().expect("tempdir should be created");
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let wal = WalStore::for_doc(tmp.path(), workspace_id, doc_id)
            .expect("per-doc wal should be created");

        let path = wal.path().to_string_lossy();
        assert!(path.contains(&workspace_id.to_string()));
        assert!(path.contains(&format!("{doc_id}.wal")));
    }

    #[cfg(unix)]
    #[test]
    fn wal_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempdir().expect("tempdir should be created");
        let wal = WalStore::open(tmp.path().join("secure.wal")).expect("wal should open");
        wal.append_update(b"payload").expect("append should succeed");

        let mode =
            std::fs::metadata(wal.path()).expect("wal metadata should load").permissions().mode()
                & 0o777;
        assert_eq!(mode, 0o600);
    }
}
