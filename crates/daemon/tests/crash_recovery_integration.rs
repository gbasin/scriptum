use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

use scriptum_common::protocol::jsonrpc::{Request, RequestId};
use scriptum_daemon::engine::ydoc::YDoc;
use scriptum_daemon::rpc::methods::{dispatch_request, RpcServerState};
use scriptum_daemon::store::snapshot::SnapshotStore;
use scriptum_daemon::store::wal::WalStore;
use serde_json::json;
use tempfile::tempdir;
use uuid::Uuid;

const WAL_FRAME_HEADER_BYTES: usize = 8;

#[tokio::test]
async fn startup_recovery_replays_after_snapshot_and_marks_checksum_failure_as_degraded() {
    let tmp = tempdir().expect("tempdir should be created");
    let crdt_store_dir = tmp.path().join("crdt_store");
    let snapshot_store = SnapshotStore::new(&crdt_store_dir).expect("snapshot store should init");

    let workspace_id = Uuid::new_v4();
    let doc_id = Uuid::new_v4();
    let wal = WalStore::for_doc(crdt_store_dir.join("wal"), workspace_id, doc_id)
        .expect("wal should initialize");

    // Live edits before crash: write 2 updates, snapshot after first.
    let live_doc = YDoc::with_client_id(1);

    let before_first = live_doc.encode_state_vector();
    live_doc.insert_text("content", 0, "hello");
    let update_1 =
        live_doc.encode_diff(&before_first).expect("first incremental update should encode");
    wal.append_update(&update_1).expect("first update should persist");
    snapshot_store
        .save_snapshot(doc_id, 1, &live_doc.encode_state())
        .expect("snapshot should persist");

    let before_second = live_doc.encode_state_vector();
    live_doc.insert_text("content", 5, " world");
    let update_2 =
        live_doc.encode_diff(&before_second).expect("second incremental update should encode");
    wal.append_update(&update_2).expect("second update should persist");

    // Simulate WAL corruption (checksum bytes) in the second frame.
    let second_frame_checksum_offset = (WAL_FRAME_HEADER_BYTES + update_1.len()) + 4;
    let mut wal_file = OpenOptions::new()
        .write(true)
        .open(wal.path())
        .expect("wal file should open for corruption");
    wal_file
        .seek(SeekFrom::Start(second_frame_checksum_offset as u64))
        .expect("seek should succeed");
    wal_file.write_all(&[0, 0, 0, 0]).expect("checksum overwrite should succeed");
    wal_file.sync_data().expect("corruption write should fsync");

    // Startup recovery should load snapshot + replay post-snapshot WAL updates.
    // The checksum failure should mark this doc degraded and keep only valid state.
    let state = RpcServerState::default();
    let report = state
        .recover_docs_at_startup(&crdt_store_dir)
        .await
        .expect("startup recovery should succeed");

    assert_eq!(report.recovered_docs, 1);
    assert_eq!(report.degraded_docs, vec![doc_id]);
    assert!(state.is_doc_degraded_for_test(doc_id).await);

    let request = Request::new(
        "doc.read",
        Some(json!({
            "workspace_id": workspace_id,
            "doc_id": doc_id,
            "include_content": true,
        })),
        RequestId::Number(1),
    );
    let response = dispatch_request(request, &state).await;
    assert!(response.error.is_none(), "expected successful doc.read: {response:?}");
    let result = response.result.expect("result should be present");

    // Snapshot state was "hello"; corrupted second update should not apply.
    assert_eq!(result["content_md"], json!("hello"));
    assert_eq!(result["degraded"], json!(true));
}
