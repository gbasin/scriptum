// Spike integration test: CLI edits doc via Y-CRDT, converges with readers.
//
// Validates the full path: CLI → JSON-RPC → doc.edit_section → Y.Doc CRDT → doc.read
//
// 1. Seed a document with markdown content.
// 2. Edit a section via doc.edit_section RPC.
// 3. Read back via doc.read and verify the edit is reflected.
// 4. Verify a second Y.Doc "client" can sync the update.

use scriptum_common::protocol::jsonrpc::{Request, RequestId};
use scriptum_daemon::engine::ydoc::YDoc;
use scriptum_daemon::rpc::methods::{dispatch_request, RpcServerState};
use serde_json::json;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────

async fn seed_doc(state: &RpcServerState, ws: Uuid, doc: Uuid, path: &str, markdown: &str) {
    state.seed_doc(ws, doc, path, path, markdown).await;
}

async fn read_content(state: &RpcServerState, ws: Uuid, doc: Uuid) -> String {
    let request = Request::new(
        "doc.read",
        Some(json!({
            "workspace_id": ws,
            "doc_id": doc,
            "include_content": true,
        })),
        RequestId::Number(99),
    );
    let response = dispatch_request(request, state).await;
    assert!(response.error.is_none(), "doc.read failed: {response:?}");
    let result = response.result.expect("result should be populated");
    result["content_md"].as_str().expect("content_md should be a string").to_string()
}

async fn edit_section(
    state: &RpcServerState,
    ws: Uuid,
    doc: Uuid,
    section: &str,
    content: &str,
    agent: &str,
) -> serde_json::Value {
    let request = Request::new(
        "doc.edit_section",
        Some(json!({
            "workspace_id": ws,
            "doc_id": doc,
            "section": section,
            "content": content,
            "agent": agent,
        })),
        RequestId::Number(100),
    );
    let response = dispatch_request(request, state).await;
    assert!(response.error.is_none(), "doc.edit_section failed: {response:?}");
    response.result.expect("result should be populated")
}

// ── Tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn edit_section_updates_crdt_content() {
    let state = RpcServerState::default();
    let ws = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let markdown =
        "# API Reference\n\n## Authentication\n\nUse API keys.\n\n## Endpoints\n\nGET /users\n";
    seed_doc(&state, ws, doc, "docs/api.md", markdown).await;

    // Edit the Authentication section.
    let result = edit_section(
        &state,
        ws,
        doc,
        "## Authentication",
        "\nUse JWT tokens with Bearer scheme.\n\n",
        "claude-1",
    )
    .await;

    assert_eq!(result["heading"], "Authentication");
    assert!(result["section_id"].as_str().unwrap().contains("authentication"));
    assert_eq!(result["doc_path"], "docs/api.md");

    // Read back and verify content changed.
    let content = read_content(&state, ws, doc).await;
    assert!(content.contains("## Authentication"), "heading should be preserved");
    assert!(content.contains("JWT tokens"), "new body should be present");
    assert!(!content.contains("API keys"), "old body should be gone");
    assert!(content.contains("## Endpoints"), "other sections should be preserved");
    assert!(content.contains("GET /users"), "other section bodies should be preserved");
}

#[tokio::test]
async fn edit_preserves_heading_and_sibling_sections() {
    let state = RpcServerState::default();
    let ws = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let markdown = "# Title\n\nIntro text.\n\n## Section A\n\nBody A.\n\n## Section B\n\nBody B.\n";
    seed_doc(&state, ws, doc, "docs/readme.md", markdown).await;

    // Edit Section A only.
    edit_section(&state, ws, doc, "## Section A", "\nNew body for A.\n\n", "alice").await;

    let content = read_content(&state, ws, doc).await;
    assert!(content.contains("# Title"), "title should remain");
    assert!(content.contains("Intro text"), "intro should remain");
    assert!(content.contains("## Section A"), "heading A should remain");
    assert!(content.contains("New body for A"), "new body A should be present");
    assert!(!content.contains("Body A."), "old body A should be gone");
    assert!(content.contains("## Section B"), "section B heading should remain");
    assert!(content.contains("Body B."), "section B body should remain");
}

#[tokio::test]
async fn edit_last_section_works() {
    let state = RpcServerState::default();
    let ws = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let markdown = "# Root\n\n## Last Section\n\nOld content.\n";
    seed_doc(&state, ws, doc, "docs/last.md", markdown).await;

    edit_section(&state, ws, doc, "## Last Section", "\nReplaced content.\n", "bob").await;

    let content = read_content(&state, ws, doc).await;
    assert!(content.contains("## Last Section"));
    assert!(content.contains("Replaced content."));
    assert!(!content.contains("Old content."));
}

#[tokio::test]
async fn edit_nonexistent_section_returns_error() {
    let state = RpcServerState::default();
    let ws = Uuid::new_v4();
    let doc = Uuid::new_v4();

    let markdown = "# Root\n\n## Exists\n\nBody.\n";
    seed_doc(&state, ws, doc, "docs/err.md", markdown).await;

    let request = Request::new(
        "doc.edit_section",
        Some(json!({
            "workspace_id": ws,
            "doc_id": doc,
            "section": "## DoesNotExist",
            "content": "new",
            "agent": "test",
        })),
        RequestId::Number(200),
    );
    let response = dispatch_request(request, &state).await;

    assert!(response.error.is_some(), "should return error for missing section");
    let error = response.error.unwrap();
    assert!(
        error.message.contains("not found"),
        "error should mention 'not found': {}",
        error.message
    );
}

#[tokio::test]
async fn crdt_update_syncs_to_second_ydoc_client() {
    // This test validates that a CRDT edit made via RPC can be synced
    // to a second Y.Doc (simulating a CodeMirror editor).

    let state = RpcServerState::default();
    let ws = Uuid::new_v4();
    let doc_id = Uuid::new_v4();

    let markdown = "# Shared Doc\n\n## Notes\n\nOriginal notes.\n";
    seed_doc(&state, ws, doc_id, "docs/shared.md", markdown).await;

    // Snapshot the CRDT state before the edit (simulating CM's initial sync).
    let pre_edit_state = {
        let manager = state.doc_manager_for_test().read().await;
        let doc = manager.get_doc(doc_id).expect("doc should exist");
        doc.encode_state()
    };

    // Simulate a "CM client" that loaded the doc before the edit.
    let cm_client = YDoc::with_client_id(42);
    cm_client.apply_update(&pre_edit_state).unwrap();
    assert_eq!(cm_client.get_text_string("content"), markdown);

    // Now apply an edit via RPC.
    edit_section(&state, ws, doc_id, "## Notes", "\nUpdated notes from CLI.\n", "cli-agent").await;

    // Get the diff from the daemon's CRDT.
    let diff = {
        let manager = state.doc_manager_for_test().read().await;
        let doc = manager.get_doc(doc_id).expect("doc should exist");
        let sv = cm_client.encode_state_vector();
        doc.encode_diff(&sv).unwrap()
    };

    // Apply the diff to the CM client's Y.Doc.
    cm_client.apply_update(&diff).unwrap();

    let cm_content = cm_client.get_text_string("content");
    assert!(cm_content.contains("Updated notes from CLI."), "CM should see the edit");
    assert!(!cm_content.contains("Original notes."), "old content should be gone from CM");
    assert!(cm_content.contains("## Notes"), "heading should be preserved in CM");
}
