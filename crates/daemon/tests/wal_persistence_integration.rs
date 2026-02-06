use scriptum_daemon::engine::ydoc::YDoc;
use scriptum_daemon::store::wal::WalStore;
use tempfile::tempdir;

#[test]
fn wal_replay_restores_document_after_restart() {
    let tmp = tempdir().expect("tempdir should be created");
    let wal_path = tmp.path().join("docs").join("doc-1.wal");

    // Simulate a running daemon: apply edits and append each Yjs update to WAL.
    let wal = WalStore::open(&wal_path).expect("wal should open");
    let live_doc = YDoc::with_client_id(1);

    let before_first = live_doc.encode_state_vector();
    live_doc.insert_text("content", 0, "hello");
    let update_1 =
        live_doc.encode_diff(&before_first).expect("first incremental update should encode");
    wal.append_update(&update_1).expect("first update should persist");

    let before_second = live_doc.encode_state_vector();
    live_doc.insert_text("content", 5, " world");
    let update_2 =
        live_doc.encode_diff(&before_second).expect("second incremental update should encode");
    wal.append_update(&update_2).expect("second update should persist");

    drop(live_doc);
    drop(wal);

    // Simulate daemon restart: replay WAL into a fresh Y.Doc instance.
    let recovered_doc = YDoc::with_client_id(2);
    let wal_after_restart = WalStore::open(&wal_path).expect("wal should reopen");
    let applied = wal_after_restart
        .replay(|update| recovered_doc.apply_update(update))
        .expect("wal replay should succeed");

    assert_eq!(applied, 2);
    assert_eq!(recovered_doc.get_text_string("content"), "hello world");
}
