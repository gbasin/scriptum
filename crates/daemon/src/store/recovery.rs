use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::engine::doc_manager::DocManager;
use crate::engine::ydoc::YDoc;
use crate::store::snapshot::SnapshotStore;
use crate::store::wal::WalStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupRecoveryReport {
    pub recovered_docs: usize,
    pub degraded_docs: Vec<Uuid>,
}

pub struct RecoveredDoc {
    pub doc: YDoc,
    pub snapshot_seq: i64,
    pub replayed_updates: usize,
    pub degraded: bool,
}

#[derive(Debug, Clone)]
struct RecoveryTarget {
    doc_id: Uuid,
    wal_path: Option<PathBuf>,
}

/// Recover all discovered documents under `crdt_store/` into the in-memory doc manager.
pub fn recover_documents_into_manager(
    crdt_store_dir: impl AsRef<Path>,
    doc_manager: &mut DocManager,
) -> Result<StartupRecoveryReport> {
    let crdt_store_dir = crdt_store_dir.as_ref();
    let snapshot_store = SnapshotStore::new(crdt_store_dir)?;
    let targets = discover_targets(crdt_store_dir)?;

    let mut degraded_docs = Vec::new();
    for target in &targets {
        let recovered =
            recover_document(&snapshot_store, target.doc_id, target.wal_path.as_deref())
                .with_context(|| format!("failed to recover doc {}", target.doc_id))?;
        if recovered.degraded {
            degraded_docs.push(target.doc_id);
        }
        doc_manager.put_doc(target.doc_id, recovered.doc);
    }
    degraded_docs.sort();

    Ok(StartupRecoveryReport { recovered_docs: targets.len(), degraded_docs })
}

/// Recover a single document from snapshot + WAL.
///
/// Recovery order:
/// 1) Load full snapshot state (if available).
/// 2) Replay WAL updates strictly after `snapshot_seq`.
/// 3) Mark degraded when WAL checksum validation fails.
pub fn recover_document(
    snapshot_store: &SnapshotStore,
    doc_id: Uuid,
    wal_path: Option<&Path>,
) -> Result<RecoveredDoc> {
    let (doc, snapshot_seq) = match snapshot_store.load_snapshot(doc_id)? {
        Some(snapshot) => {
            let doc = YDoc::from_state(&snapshot.payload)
                .with_context(|| format!("failed to load snapshot state for doc {}", doc_id))?;
            (doc, snapshot.snapshot_seq.max(0))
        }
        None => (YDoc::new(), 0),
    };

    let mut replayed_updates = 0usize;
    let mut degraded = false;
    if let Some(path) = wal_path {
        if path.exists() {
            let wal = WalStore::open(path)?;
            let start_frame = usize::try_from(snapshot_seq).unwrap_or(usize::MAX);
            let summary = wal.replay_from_frame(start_frame, |update| doc.apply_update(update))?;
            replayed_updates = summary.applied;
            degraded = summary.checksum_failed;
        }
    }

    Ok(RecoveredDoc { doc, snapshot_seq, replayed_updates, degraded })
}

fn discover_targets(crdt_store_dir: &Path) -> Result<Vec<RecoveryTarget>> {
    let mut targets: BTreeMap<Uuid, Option<PathBuf>> = BTreeMap::new();

    let snapshots_dir = crdt_store_dir.join("snapshots");
    if snapshots_dir.exists() {
        for path in read_files_sorted(&snapshots_dir)? {
            if let Some(doc_id) = parse_doc_id(&path, "snap") {
                targets.entry(doc_id).or_insert(None);
            }
        }
    }

    let wal_root = crdt_store_dir.join("wal");
    if wal_root.exists() {
        for workspace_dir in read_dirs_sorted(&wal_root)? {
            for wal_path in read_files_sorted(&workspace_dir)? {
                if let Some(doc_id) = parse_doc_id(&wal_path, "wal") {
                    targets
                        .entry(doc_id)
                        .and_modify(|current| {
                            if current.is_none() {
                                *current = Some(wal_path.clone());
                            }
                        })
                        .or_insert_with(|| Some(wal_path.clone()));
                }
            }
        }
    }

    Ok(targets.into_iter().map(|(doc_id, wal_path)| RecoveryTarget { doc_id, wal_path }).collect())
}

fn parse_doc_id(path: &Path, ext: &str) -> Option<Uuid> {
    if path.extension().and_then(|value| value.to_str()) != Some(ext) {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    Uuid::parse_str(stem).ok()
}

fn read_dirs_sorted(path: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = fs::read_dir(path)
        .with_context(|| format!("failed to read directory `{}`", path.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to iterate directory `{}`", path.display()))?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|entry| entry.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    Ok(dirs)
}

fn read_files_sorted(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = fs::read_dir(path)
        .with_context(|| format!("failed to read directory `{}`", path.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to iterate directory `{}`", path.display()))?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|entry| entry.is_file())
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}
