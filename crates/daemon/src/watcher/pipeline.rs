// Full file watcher pipeline: FS event → debounce → read → hash → diff → Yjs.
//
// Connects the watcher stages into a single async pipeline that converts
// external file edits into CRDT updates.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex as StdMutex};
use std::time::Duration;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use tokio::sync::mpsc;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use scriptum_common::backlink::parse_wiki_links;
use scriptum_common::diff::patch;

use crate::engine::doc_manager::DocManager;
use crate::engine::ydoc::YDoc;

use super::debounce::{DebounceConfig, Debouncer};
use super::hash;
use super::{FsEventKind, RawFsEvent};

/// Suppresses watcher events briefly after daemon-initiated writes.
///
/// This prevents feedback loops where a remote CRDT update writes to disk and
/// the watcher immediately re-processes the same content as a local edit.
#[derive(Debug, Default)]
pub struct WatcherPauseController {
    paused_until: StdMutex<std::collections::HashMap<PathBuf, Instant>>,
}

impl WatcherPauseController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pause watcher processing for `path` for `duration`.
    pub fn pause_path_for(&self, path: &Path, duration: Duration) {
        let mut guard = self.paused_until.lock().expect("watcher pause lock poisoned");
        guard.insert(path.to_path_buf(), Instant::now() + duration);
    }

    /// Returns true when the path is currently paused.
    pub fn is_paused(&self, path: &Path) -> bool {
        let now = Instant::now();
        let mut guard = self.paused_until.lock().expect("watcher pause lock poisoned");
        guard.retain(|_, until| *until > now);
        guard.get(path).is_some_and(|until| *until > now)
    }
}

/// How we resolve an absolute file path to a document identity.
pub trait PathResolver: Send + Sync {
    /// Map a file path to a (workspace_id, doc_id) pair.
    /// Returns None if the path is not within any tracked workspace.
    fn resolve(&self, path: &Path) -> Option<(Uuid, Uuid)>;
}

/// Trait for persisting content hashes (abstracts SQLite for testing).
pub trait HashStore: Send + Sync {
    /// Get the stored hash for a doc_id. None if not tracked.
    fn get_hash(&self, doc_id: &str) -> Result<Option<String>>;
    /// Update the stored hash for a doc_id.
    fn set_hash(&self, doc_id: &str, hash: &str) -> Result<()>;
}

/// Events produced by the pipeline for upstream consumers.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineEvent {
    /// A document was updated from a local file change.
    DocUpdated {
        workspace_id: Uuid,
        doc_id: Uuid,
        path: PathBuf,
        content_hash: String,
        patch_op_count: usize,
    },
    /// A document was removed (file deleted).
    DocRemoved { workspace_id: Uuid, doc_id: Uuid, path: PathBuf },
    /// An error occurred processing a file event.
    Error { path: PathBuf, error: String },
}

/// Configuration for the watcher pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub debounce: DebounceConfig,
    /// How often to check the debouncer for ready events (poll interval).
    pub poll_interval: Duration,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self { debounce: DebounceConfig::default(), poll_interval: Duration::from_millis(50) }
    }
}

/// Runs the full watcher pipeline as an async loop.
///
/// Consumes raw FS events from the watcher, debounces them, and applies
/// file changes to the CRDT doc manager. Emits `PipelineEvent`s for
/// upstream consumers (outbox, UI).
///
/// Exits when `raw_rx` closes (watcher dropped) or `shutdown` fires.
pub async fn run_pipeline(
    mut raw_rx: mpsc::Receiver<RawFsEvent>,
    event_tx: mpsc::Sender<PipelineEvent>,
    doc_manager: Arc<tokio::sync::Mutex<DocManager>>,
    resolver: Arc<dyn PathResolver>,
    hash_store: Arc<dyn HashStore>,
    pause_controller: Option<Arc<WatcherPauseController>>,
    config: PipelineConfig,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) {
    let mut debouncer = Debouncer::new(config.debounce);

    info!("watcher pipeline started");

    loop {
        tokio::select! {
            biased;

            _ = shutdown.recv() => {
                info!("watcher pipeline shutting down");
                break;
            }

            maybe_event = raw_rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        trace!(path = %event.path.display(), kind = ?event.kind, "raw event received");
                        debouncer.push(event);
                    }
                    None => {
                        info!("raw event channel closed, pipeline exiting");
                        break;
                    }
                }
            }

            _ = tokio::time::sleep(config.poll_interval) => {
                // Check for ready debounced events.
            }
        }

        // Drain any events that have passed the debounce window.
        let ready = debouncer.drain_ready();
        for event in ready {
            let result = if let Some(controller) = pause_controller.as_deref() {
                process_event_with_pause(
                    &event,
                    &doc_manager,
                    resolver.as_ref(),
                    hash_store.as_ref(),
                    Some(controller),
                )
                .await
            } else {
                process_event(
                    &event,
                    &doc_manager,
                    resolver.as_ref(),
                    hash_store.as_ref(),
                )
                .await
            };

            let pipeline_event = match result {
                Ok(Some(pe)) => pe,
                Ok(None) => {
                    trace!(path = %event.path.display(), "no-op (hash unchanged)");
                    continue;
                }
                Err(e) => {
                    warn!(path = %event.path.display(), error = %e, "pipeline error");
                    PipelineEvent::Error { path: event.path, error: e.to_string() }
                }
            };

            if event_tx.send(pipeline_event).await.is_err() {
                debug!("pipeline event channel closed, exiting");
                return;
            }
        }
    }
}

/// Process a single debounced event.
async fn process_event(
    event: &RawFsEvent,
    doc_manager: &Arc<tokio::sync::Mutex<DocManager>>,
    resolver: &dyn PathResolver,
    hash_store: &dyn HashStore,
) -> Result<Option<PipelineEvent>> {
    process_event_with_pause(event, doc_manager, resolver, hash_store, None).await
}

async fn process_event_with_pause(
    event: &RawFsEvent,
    doc_manager: &Arc<tokio::sync::Mutex<DocManager>>,
    resolver: &dyn PathResolver,
    hash_store: &dyn HashStore,
    pause_controller: Option<&WatcherPauseController>,
) -> Result<Option<PipelineEvent>> {
    if matches!(event.kind, FsEventKind::Create | FsEventKind::Modify)
        && pause_controller.is_some_and(|controller| controller.is_paused(&event.path))
    {
        trace!(path = %event.path.display(), "skipping watcher event for paused path");
        return Ok(None);
    }

    let (workspace_id, doc_id) = resolver
        .resolve(&event.path)
        .ok_or_else(|| anyhow!("path not in any workspace: {}", event.path.display()))?;

    let doc_id_str = doc_id.to_string();

    match event.kind {
        FsEventKind::Remove => {
            Ok(Some(PipelineEvent::DocRemoved { workspace_id, doc_id, path: event.path.clone() }))
        }

        FsEventKind::Create | FsEventKind::Modify => {
            // Read file content.
            let content = std::fs::read_to_string(&event.path)
                .with_context(|| format!("failed to read {}", event.path.display()))?;

            // Save-time backlink extraction (indexing is handled by downstream stages).
            let _wiki_links = parse_wiki_links(&content);

            // Hash check — skip if unchanged.
            let new_hash = hash::sha256_hex(content.as_bytes());
            let stored = hash_store.get_hash(&doc_id_str)?;
            if stored.as_deref() == Some(new_hash.as_str()) {
                return Ok(None); // No-op save.
            }

            // Get or create CRDT document.
            let mut mgr = doc_manager.lock().await;
            let doc: Arc<YDoc> = mgr.subscribe_or_create(doc_id);
            let current_text = doc.get_text_string("content");

            // Diff and apply.
            let ytext = doc.get_or_insert_text("content");
            let ops = patch::apply_text_diff_to_ytext(doc.inner(), &ytext, &current_text, &content);
            let op_count = ops.len();
            drop(mgr); // Release lock before I/O.

            // Update stored hash.
            let _ = hash_store.set_hash(&doc_id_str, &new_hash);

            Ok(Some(PipelineEvent::DocUpdated {
                workspace_id,
                doc_id,
                path: event.path.clone(),
                content_hash: new_hash,
                patch_op_count: op_count,
            }))
        }
    }
}

/// Apply a remote CRDT update and materialize it to the markdown file on disk.
///
/// Returns `Ok(true)` when the file was written, `Ok(false)` when on-disk
/// content already matched the CRDT-rendered markdown.
pub async fn apply_remote_update_to_disk(
    doc_id: Uuid,
    path: &Path,
    update: &[u8],
    doc_manager: &Arc<tokio::sync::Mutex<DocManager>>,
    hash_store: &dyn HashStore,
    pause_controller: Option<&WatcherPauseController>,
    watcher_pause_duration: Duration,
) -> Result<bool> {
    let doc = {
        let mut manager = doc_manager.lock().await;
        manager.subscribe_or_create(doc_id)
    };

    doc.apply_update(update).context("failed to apply remote CRDT update")?;
    let rendered_markdown = doc.get_text_string("content");

    {
        let mut manager = doc_manager.lock().await;
        let _ = manager.unsubscribe(doc_id);
    }

    let existing_markdown = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to read current markdown file `{}`", path.display())
            })
        }
    };

    if existing_markdown == rendered_markdown {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create parent directory `{}` for remote write", parent.display())
        })?;
    }

    if let Some(controller) = pause_controller {
        controller.pause_path_for(path, watcher_pause_duration);
    }

    std::fs::write(path, rendered_markdown.as_bytes())
        .with_context(|| format!("failed to write remote markdown to `{}`", path.display()))?;

    let new_hash = hash::sha256_hex(rendered_markdown.as_bytes());
    hash_store.set_hash(&doc_id.to_string(), &new_hash)?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex as StdMutex};

    use tokio::sync::{broadcast, mpsc, Mutex};
    use uuid::Uuid;

    use crate::engine::doc_manager::DocManager;

    use super::*;

    // ── Mock PathResolver ──────────────────────────────────────────

    struct MockResolver {
        workspace_id: Uuid,
        mappings: HashMap<PathBuf, Uuid>,
    }

    impl MockResolver {
        fn new(workspace_id: Uuid) -> Self {
            Self { workspace_id, mappings: HashMap::new() }
        }

        fn add(&mut self, path: &str, doc_id: Uuid) {
            self.mappings.insert(PathBuf::from(path), doc_id);
        }
    }

    impl PathResolver for MockResolver {
        fn resolve(&self, path: &Path) -> Option<(Uuid, Uuid)> {
            self.mappings.get(path).map(|doc_id| (self.workspace_id, *doc_id))
        }
    }

    // ── Mock HashStore ─────────────────────────────────────────────

    #[derive(Default)]
    struct MockHashStore {
        hashes: StdMutex<HashMap<String, String>>,
    }

    impl HashStore for MockHashStore {
        fn get_hash(&self, doc_id: &str) -> Result<Option<String>> {
            Ok(self.hashes.lock().unwrap().get(doc_id).cloned())
        }

        fn set_hash(&self, doc_id: &str, hash: &str) -> Result<()> {
            self.hashes.lock().unwrap().insert(doc_id.to_string(), hash.to_string());
            Ok(())
        }
    }

    // ── Test helpers ───────────────────────────────────────────────

    fn setup() -> (
        Uuid,    // workspace_id
        Uuid,    // doc_id
        PathBuf, // file_path
        Arc<dyn PathResolver>,
        Arc<dyn HashStore>,
        Arc<Mutex<DocManager>>,
    ) {
        let ws_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let path = PathBuf::from("/workspace/test.md");

        let mut resolver = MockResolver::new(ws_id);
        resolver.add("/workspace/test.md", doc_id);

        let hash_store = Arc::new(MockHashStore::default());
        let doc_mgr = Arc::new(Mutex::new(DocManager::new(64 * 1024 * 1024)));

        (ws_id, doc_id, path, Arc::new(resolver), hash_store, doc_mgr)
    }

    // ── process_event tests ────────────────────────────────────────

    #[tokio::test]
    async fn create_event_creates_doc_and_emits_update() {
        let (ws_id, doc_id, _path, _resolver, hash_store, doc_mgr) = setup();

        // Write a file for the event to read.
        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("test.md");
        std::fs::write(&file_path, "# Hello\n").unwrap();

        // Re-create resolver with real path.
        let mut resolver_mut = MockResolver::new(ws_id);
        resolver_mut.add(file_path.to_str().unwrap(), doc_id);
        let resolver: Arc<dyn PathResolver> = Arc::new(resolver_mut);

        let event = RawFsEvent { kind: FsEventKind::Create, path: file_path.clone() };

        let result =
            process_event(&event, &doc_mgr, resolver.as_ref(), hash_store.as_ref()).await.unwrap();

        match result {
            Some(PipelineEvent::DocUpdated {
                workspace_id,
                doc_id: did,
                content_hash,
                patch_op_count,
                ..
            }) => {
                assert_eq!(workspace_id, ws_id);
                assert_eq!(did, doc_id);
                assert_eq!(content_hash, hash::sha256_hex(b"# Hello\n"));
                assert!(patch_op_count > 0);
            }
            other => panic!("expected DocUpdated, got {other:?}"),
        }

        // Verify CRDT state.
        let mut mgr = doc_mgr.lock().await;
        let doc = mgr.subscribe_or_create(doc_id);
        assert_eq!(doc.get_text_string("content"), "# Hello\n");
    }

    #[tokio::test]
    async fn modify_with_same_hash_is_noop() {
        let (ws_id, doc_id, _path, _resolver, hash_store, doc_mgr) = setup();

        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("test.md");
        std::fs::write(&file_path, "# Same\n").unwrap();

        let mut resolver = MockResolver::new(ws_id);
        resolver.add(file_path.to_str().unwrap(), doc_id);
        let resolver: Arc<dyn PathResolver> = Arc::new(resolver);

        // Pre-store the hash.
        let h = hash::sha256_hex(b"# Same\n");
        hash_store.set_hash(&doc_id.to_string(), &h).unwrap();

        let event = RawFsEvent { kind: FsEventKind::Modify, path: file_path };
        let result =
            process_event(&event, &doc_mgr, resolver.as_ref(), hash_store.as_ref()).await.unwrap();

        assert!(result.is_none(), "unchanged content should be no-op");
    }

    #[tokio::test]
    async fn remove_event_emits_doc_removed() {
        let (ws_id, doc_id, path, resolver, hash_store, doc_mgr) = setup();

        let event = RawFsEvent { kind: FsEventKind::Remove, path: path.clone() };
        let result =
            process_event(&event, &doc_mgr, resolver.as_ref(), hash_store.as_ref()).await.unwrap();

        match result {
            Some(PipelineEvent::DocRemoved { workspace_id, doc_id: did, path: p }) => {
                assert_eq!(workspace_id, ws_id);
                assert_eq!(did, doc_id);
                assert_eq!(p, path);
            }
            other => panic!("expected DocRemoved, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_path_returns_error() {
        let (_ws_id, _doc_id, _path, _resolver, hash_store, doc_mgr) = setup();

        // Empty resolver — no paths mapped.
        let resolver: Arc<dyn PathResolver> = Arc::new(MockResolver::new(Uuid::new_v4()));

        let event =
            RawFsEvent { kind: FsEventKind::Modify, path: PathBuf::from("/unknown/file.md") };

        let result = process_event(&event, &doc_mgr, resolver.as_ref(), hash_store.as_ref()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn modify_updates_existing_doc_content() {
        let (ws_id, doc_id, _path, _resolver, hash_store, doc_mgr) = setup();

        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("test.md");

        let mut resolver = MockResolver::new(ws_id);
        resolver.add(file_path.to_str().unwrap(), doc_id);
        let resolver: Arc<dyn PathResolver> = Arc::new(resolver);

        // Create initial content via the pipeline.
        std::fs::write(&file_path, "# First\n").unwrap();
        let event = RawFsEvent { kind: FsEventKind::Create, path: file_path.clone() };
        process_event(&event, &doc_mgr, resolver.as_ref(), hash_store.as_ref()).await.unwrap();

        // Modify.
        std::fs::write(&file_path, "# Updated\n").unwrap();
        let event = RawFsEvent { kind: FsEventKind::Modify, path: file_path.clone() };
        let result =
            process_event(&event, &doc_mgr, resolver.as_ref(), hash_store.as_ref()).await.unwrap();

        assert!(result.is_some());
        let mut mgr = doc_mgr.lock().await;
        let doc = mgr.subscribe_or_create(doc_id);
        assert_eq!(doc.get_text_string("content"), "# Updated\n");
    }

    #[tokio::test]
    async fn hash_store_is_updated_after_successful_apply() {
        let (ws_id, doc_id, _path, _resolver, hash_store, doc_mgr) = setup();

        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("test.md");
        std::fs::write(&file_path, "content").unwrap();

        let mut resolver = MockResolver::new(ws_id);
        resolver.add(file_path.to_str().unwrap(), doc_id);
        let resolver: Arc<dyn PathResolver> = Arc::new(resolver);

        let event = RawFsEvent { kind: FsEventKind::Create, path: file_path };
        process_event(&event, &doc_mgr, resolver.as_ref(), hash_store.as_ref()).await.unwrap();

        let stored = hash_store.get_hash(&doc_id.to_string()).unwrap();
        assert_eq!(stored, Some(hash::sha256_hex(b"content")));
    }

    #[tokio::test]
    async fn remote_update_writes_markdown_to_disk() {
        let (ws_id, doc_id, _path, _resolver, hash_store, doc_mgr) = setup();

        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("remote.md");
        let mut resolver = MockResolver::new(ws_id);
        resolver.add(file_path.to_str().unwrap(), doc_id);
        let resolver: Arc<dyn PathResolver> = Arc::new(resolver);

        let remote_doc = YDoc::new();
        remote_doc.insert_text("content", 0, "# Remote\n\nMerged from relay.\n");
        let remote_update = remote_doc.encode_state();

        let wrote = apply_remote_update_to_disk(
            doc_id,
            &file_path,
            &remote_update,
            &doc_mgr,
            hash_store.as_ref(),
            None,
            Duration::from_millis(250),
        )
        .await
        .unwrap();
        assert!(wrote, "remote update should write markdown file");

        let on_disk = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(on_disk, "# Remote\n\nMerged from relay.\n");

        // A watcher modify event for the same content should be a no-op via hash match.
        let event = RawFsEvent { kind: FsEventKind::Modify, path: file_path };
        let result =
            process_event(&event, &doc_mgr, resolver.as_ref(), hash_store.as_ref()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn paused_path_skips_followup_watcher_event_after_remote_write() {
        let (ws_id, doc_id, _path, _resolver, hash_store, doc_mgr) = setup();

        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("paused.md");
        let mut resolver = MockResolver::new(ws_id);
        resolver.add(file_path.to_str().unwrap(), doc_id);
        let resolver: Arc<dyn PathResolver> = Arc::new(resolver);
        let pause_controller = WatcherPauseController::new();

        let remote_doc = YDoc::new();
        remote_doc.insert_text("content", 0, "# Paused\n");
        let remote_update = remote_doc.encode_state();

        let wrote = apply_remote_update_to_disk(
            doc_id,
            &file_path,
            &remote_update,
            &doc_mgr,
            hash_store.as_ref(),
            Some(&pause_controller),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(wrote);

        let event = RawFsEvent { kind: FsEventKind::Modify, path: file_path };
        let result = process_event_with_pause(
            &event,
            &doc_mgr,
            resolver.as_ref(),
            hash_store.as_ref(),
            Some(&pause_controller),
        )
        .await
        .unwrap();
        assert!(result.is_none(), "paused path should suppress watcher feedback event");
    }

    // ── Pipeline integration test ──────────────────────────────────

    #[tokio::test]
    async fn pipeline_processes_events_end_to_end() {
        let ws_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("test.md");
        std::fs::write(&file_path, "# End to End\n").unwrap();

        let mut resolver = MockResolver::new(ws_id);
        resolver.add(file_path.to_str().unwrap(), doc_id);

        let hash_store = Arc::new(MockHashStore::default());
        let doc_mgr = Arc::new(Mutex::new(DocManager::new(64 * 1024 * 1024)));
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let (raw_tx, raw_rx) = mpsc::channel(32);
        let (event_tx, mut event_rx) = mpsc::channel(32);

        let config = PipelineConfig {
            debounce: DebounceConfig::with_millis(50),
            poll_interval: Duration::from_millis(10),
        };

        let resolver_arc: Arc<dyn PathResolver> = Arc::new(resolver);
        let hash_store_arc = hash_store.clone();
        let doc_mgr_clone = doc_mgr.clone();

        // Start pipeline in background.
        let pipeline_handle = tokio::spawn(async move {
            run_pipeline(
                raw_rx,
                event_tx,
                doc_mgr_clone,
                resolver_arc,
                hash_store_arc,
                None,
                config,
                shutdown_rx,
            )
            .await;
        });

        // Send a raw event.
        raw_tx
            .send(RawFsEvent { kind: FsEventKind::Create, path: file_path.clone() })
            .await
            .unwrap();

        // Wait for the pipeline event (debounce + processing).
        let pe = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timed out waiting for pipeline event")
            .expect("channel closed");

        match pe {
            PipelineEvent::DocUpdated { workspace_id, doc_id: did, content_hash, .. } => {
                assert_eq!(workspace_id, ws_id);
                assert_eq!(did, doc_id);
                assert_eq!(content_hash, hash::sha256_hex(b"# End to End\n"));
            }
            other => panic!("expected DocUpdated, got {other:?}"),
        }

        // Verify CRDT state.
        let mut mgr = doc_mgr.lock().await;
        let doc = mgr.subscribe_or_create(doc_id);
        assert_eq!(doc.get_text_string("content"), "# End to End\n");
        drop(mgr);

        // Shutdown.
        let _ = shutdown_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(2), pipeline_handle).await;
    }
}
