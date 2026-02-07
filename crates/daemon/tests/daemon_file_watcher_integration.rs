// Integration test: daemon + file watcher bidirectional pipeline.
//
// 1. Set up a file watcher pipeline pointing at a temp directory.
// 2. Write a .md file to disk → verify CRDT document is updated.
// 3. Modify the file → verify CRDT reflects the change.
// 4. Verify RPC doc.read returns the correct content.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use scriptum_common::protocol::jsonrpc::{Request, RequestId};
use scriptum_daemon::engine::doc_manager::DocManager;
use scriptum_daemon::rpc::methods::RpcServerState;
use scriptum_daemon::watcher::debounce::DebounceConfig;
use scriptum_daemon::watcher::pipeline::{
    run_pipeline, HashStore, PathResolver, PipelineConfig, PipelineEvent,
};
use scriptum_daemon::watcher::{FsEventKind, RawFsEvent};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, Mutex};
use uuid::Uuid;

// ── Mock PathResolver ────────────────────────────────────────────────

struct TestResolver {
    workspace_id: Uuid,
    mappings: StdMutex<HashMap<PathBuf, Uuid>>,
}

impl TestResolver {
    fn new(workspace_id: Uuid) -> Self {
        Self { workspace_id, mappings: StdMutex::new(HashMap::new()) }
    }

    fn register(&self, path: &Path, doc_id: Uuid) {
        self.mappings.lock().unwrap().insert(path.to_path_buf(), doc_id);
    }
}

impl PathResolver for TestResolver {
    fn resolve(&self, path: &Path) -> Option<(Uuid, Uuid)> {
        self.mappings.lock().unwrap().get(path).map(|doc_id| (self.workspace_id, *doc_id))
    }
}

// ── Mock HashStore ───────────────────────────────────────────────────

#[derive(Default)]
struct TestHashStore {
    hashes: StdMutex<HashMap<String, String>>,
}

impl HashStore for TestHashStore {
    fn get_hash(&self, doc_id: &str) -> anyhow::Result<Option<String>> {
        Ok(self.hashes.lock().unwrap().get(doc_id).cloned())
    }

    fn set_hash(&self, doc_id: &str, hash: &str) -> anyhow::Result<()> {
        self.hashes.lock().unwrap().insert(doc_id.to_string(), hash.to_string());
        Ok(())
    }
}

// ── Test infrastructure ──────────────────────────────────────────────

struct TestHarness {
    workspace_id: Uuid,
    doc_id: Uuid,
    file_path: PathBuf,
    raw_tx: mpsc::Sender<RawFsEvent>,
    event_rx: mpsc::Receiver<PipelineEvent>,
    doc_manager: Arc<Mutex<DocManager>>,
    rpc_state: RpcServerState,
    _shutdown_tx: broadcast::Sender<()>,
}

fn setup_harness(tmp: &tempfile::TempDir) -> TestHarness {
    let workspace_id = Uuid::new_v4();
    let doc_id = Uuid::new_v4();
    let file_path = tmp.path().join("docs").join("readme.md");

    // Create parent directory.
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();

    // Set up resolver.
    let resolver = Arc::new(TestResolver::new(workspace_id));
    resolver.register(&file_path, doc_id);

    let hash_store: Arc<dyn HashStore> = Arc::new(TestHashStore::default());
    let doc_manager = Arc::new(Mutex::new(DocManager::new(64 * 1024 * 1024)));

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let (raw_tx, raw_rx) = mpsc::channel(64);
    let (event_tx, event_rx) = mpsc::channel(64);

    let config = PipelineConfig {
        debounce: DebounceConfig::with_millis(20),
        poll_interval: Duration::from_millis(5),
    };

    // Start pipeline in background.
    let doc_mgr_clone = doc_manager.clone();
    tokio::spawn(async move {
        run_pipeline(
            raw_rx,
            event_tx,
            doc_mgr_clone,
            resolver,
            hash_store,
            None,
            config,
            shutdown_rx,
        )
        .await;
    });

    // Set up RPC state for doc.read testing.
    let rpc_state = RpcServerState::default();

    TestHarness {
        workspace_id,
        doc_id,
        file_path,
        raw_tx,
        event_rx,
        doc_manager,
        rpc_state,
        _shutdown_tx: shutdown_tx,
    }
}

async fn wait_for_update(event_rx: &mut mpsc::Receiver<PipelineEvent>) -> PipelineEvent {
    tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("timed out waiting for pipeline event")
        .expect("pipeline channel closed")
}

// ── Tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn file_write_updates_crdt_document() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut h = setup_harness(&tmp);

    // Step 1: Write a markdown file.
    std::fs::write(&h.file_path, "# Welcome\n\nHello world.\n").unwrap();

    // Step 2: Send a create event (simulating what the OS watcher would emit).
    h.raw_tx
        .send(RawFsEvent { kind: FsEventKind::Create, path: h.file_path.clone() })
        .await
        .unwrap();

    // Step 3: Wait for pipeline to process.
    let event = wait_for_update(&mut h.event_rx).await;
    match &event {
        PipelineEvent::DocUpdated { workspace_id, doc_id, patch_op_count, .. } => {
            assert_eq!(*workspace_id, h.workspace_id);
            assert_eq!(*doc_id, h.doc_id);
            assert!(*patch_op_count > 0, "should have applied at least one patch op");
        }
        other => panic!("expected DocUpdated, got {other:?}"),
    }

    // Step 4: Verify CRDT state via doc_manager.
    let crdt_text = {
        let mut mgr = h.doc_manager.lock().await;
        let doc = mgr.subscribe_or_create(h.doc_id);
        let text = doc.get_text_string("content");
        mgr.unsubscribe(h.doc_id);
        text
    };
    assert_eq!(crdt_text, "# Welcome\n\nHello world.\n");
}

#[tokio::test]
async fn file_modify_updates_existing_crdt_content() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut h = setup_harness(&tmp);

    // Create initial file.
    std::fs::write(&h.file_path, "# Draft\n").unwrap();
    h.raw_tx
        .send(RawFsEvent { kind: FsEventKind::Create, path: h.file_path.clone() })
        .await
        .unwrap();
    let _ = wait_for_update(&mut h.event_rx).await;

    // Verify initial state.
    {
        let mut mgr = h.doc_manager.lock().await;
        let doc = mgr.subscribe_or_create(h.doc_id);
        assert_eq!(doc.get_text_string("content"), "# Draft\n");
        mgr.unsubscribe(h.doc_id);
    }

    // Modify the file.
    std::fs::write(&h.file_path, "# Draft\n\n## Section A\n\nContent here.\n").unwrap();
    h.raw_tx
        .send(RawFsEvent { kind: FsEventKind::Modify, path: h.file_path.clone() })
        .await
        .unwrap();
    let _ = wait_for_update(&mut h.event_rx).await;

    // Verify updated CRDT state.
    let crdt_text = {
        let mut mgr = h.doc_manager.lock().await;
        let doc = mgr.subscribe_or_create(h.doc_id);
        let text = doc.get_text_string("content");
        mgr.unsubscribe(h.doc_id);
        text
    };
    assert_eq!(crdt_text, "# Draft\n\n## Section A\n\nContent here.\n");
}

#[tokio::test]
async fn rpc_doc_read_returns_content_from_seeded_doc() {
    let tmp = tempfile::TempDir::new().unwrap();
    let h = setup_harness(&tmp);

    // Seed a doc directly into RPC state.
    h.rpc_state
        .seed_doc(
            h.workspace_id,
            h.doc_id,
            "docs/readme.md",
            "README",
            "# README\n\n## Getting Started\n\nInstall dependencies.\n",
        )
        .await;

    // Verify via doc.read RPC.
    let request = Request::new(
        "doc.read",
        Some(json!({
            "workspace_id": h.workspace_id,
            "doc_id": h.doc_id,
            "include_content": true,
        })),
        RequestId::Number(1),
    );

    let response = scriptum_daemon::rpc::methods::dispatch_request(request, &h.rpc_state).await;

    assert!(response.error.is_none(), "expected success: {response:?}");
    let result = response.result.expect("result should be populated");
    assert_eq!(result["metadata"]["path"], json!("docs/readme.md"));
    assert_eq!(result["metadata"]["title"], json!("README"));
    assert_eq!(
        result["content_md"],
        json!("# README\n\n## Getting Started\n\nInstall dependencies.\n")
    );

    // Verify sections were parsed.
    let sections = result["sections"].as_array().expect("sections should be an array");
    assert_eq!(sections.len(), 2, "should find 2 sections (# README, ## Getting Started)");
}

#[tokio::test]
async fn file_delete_emits_doc_removed_event() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut h = setup_harness(&tmp);

    // Create file first.
    std::fs::write(&h.file_path, "# To Delete\n").unwrap();
    h.raw_tx
        .send(RawFsEvent { kind: FsEventKind::Create, path: h.file_path.clone() })
        .await
        .unwrap();
    let _ = wait_for_update(&mut h.event_rx).await;

    // Delete the file.
    std::fs::remove_file(&h.file_path).unwrap();
    h.raw_tx
        .send(RawFsEvent { kind: FsEventKind::Remove, path: h.file_path.clone() })
        .await
        .unwrap();
    let event = wait_for_update(&mut h.event_rx).await;

    match &event {
        PipelineEvent::DocRemoved { workspace_id, doc_id, path } => {
            assert_eq!(*workspace_id, h.workspace_id);
            assert_eq!(*doc_id, h.doc_id);
            assert_eq!(*path, h.file_path);
        }
        other => panic!("expected DocRemoved, got {other:?}"),
    }
}

#[tokio::test]
async fn pipeline_and_rpc_share_doc_state() {
    // Test the full bidirectional flow:
    // 1. Write file → pipeline updates CRDT
    // 2. Read CRDT content via doc_manager (simulating what RPC would use)
    // 3. Verify section parsing works on the CRDT content

    let tmp = tempfile::TempDir::new().unwrap();
    let mut h = setup_harness(&tmp);

    let markdown = "# API Reference\n\n## Authentication\n\nUse JWT tokens.\n\n## Endpoints\n\n### GET /users\n\nReturns user list.\n";
    std::fs::write(&h.file_path, markdown).unwrap();
    h.raw_tx
        .send(RawFsEvent { kind: FsEventKind::Create, path: h.file_path.clone() })
        .await
        .unwrap();
    let _ = wait_for_update(&mut h.event_rx).await;

    // Read from CRDT.
    let crdt_text = {
        let mut mgr = h.doc_manager.lock().await;
        let doc = mgr.subscribe_or_create(h.doc_id);
        let text = doc.get_text_string("content");
        mgr.unsubscribe(h.doc_id);
        text
    };
    assert_eq!(crdt_text, markdown);

    // Parse sections from CRDT content (as RPC doc.read would).
    let sections = scriptum_common::section::parser::parse_sections(&crdt_text);
    assert_eq!(sections.len(), 4, "should find 4 sections");
    assert_eq!(sections[0].heading, "API Reference");
    assert_eq!(sections[1].heading, "Authentication");
    assert_eq!(sections[2].heading, "Endpoints");
    assert_eq!(sections[3].heading, "GET /users");
}
