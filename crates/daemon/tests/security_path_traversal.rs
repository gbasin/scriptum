use scriptum_common::path::normalize::PathError;
use scriptum_common::path::normalize_path;
use scriptum_common::protocol::jsonrpc::{Request, RequestId};
use scriptum_daemon::rpc::methods::{dispatch_request, RpcServerState};
use serde_json::json;
use uuid::Uuid;

#[test]
fn rejects_parent_directory_traversal_sequences() {
    assert_eq!(
        normalize_path("../../../etc/passwd"),
        Err(PathError::Traversal("..".to_string()))
    );
    assert_eq!(
        normalize_path("docs/../secrets.md"),
        Err(PathError::Traversal("..".to_string()))
    );
}

#[test]
fn rejects_null_bytes_and_overlong_paths() {
    assert_eq!(normalize_path("docs/file\0.md"), Err(PathError::NullByte));
    assert_eq!(normalize_path(&"a".repeat(513)), Err(PathError::TooLong));
}

#[test]
fn normalizes_unicode_equivalents_to_same_path() {
    let decomposed = normalize_path("docs/caf\u{0065}\u{0301}.md").expect("path should normalize");
    let composed = normalize_path("docs/café.md").expect("path should normalize");
    assert_eq!(decomposed, composed);
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_import_ignores_symlink_escape_attempts() {
    use std::os::unix::fs::symlink;

    let state = RpcServerState::default();
    let workspace_root = tempfile::tempdir().expect("workspace tempdir should be created");
    let outside_root = tempfile::tempdir().expect("outside tempdir should be created");
    let outside_file = outside_root.path().join("outside.md");
    std::fs::write(&outside_file, "# outside").expect("outside markdown should be written");

    let link_path = workspace_root.path().join("escape.md");
    symlink(&outside_file, &link_path).expect("symlink should be created");

    let create_request = Request::new(
        "workspace.create",
        Some(json!({
            "name": "Traversal Security",
            "root_path": workspace_root.path().to_string_lossy(),
        })),
        RequestId::Number(901),
    );
    let create_response = dispatch_request(create_request, &state).await;
    assert!(
        create_response.error.is_none(),
        "workspace.create should succeed: {create_response:?}"
    );

    let result = create_response
        .result
        .expect("workspace.create result should exist");
    let workspace_id: Uuid = serde_json::from_value(
        result["workspace_id"].clone(),
    )
    .expect("workspace_id should decode");

    let tree_request = Request::new(
        "doc.tree",
        Some(json!({ "workspace_id": workspace_id })),
        RequestId::Number(902),
    );
    let tree_response = dispatch_request(tree_request, &state).await;
    assert!(
        tree_response.error.is_none(),
        "doc.tree should succeed: {tree_response:?}"
    );
    let tree = tree_response.result.expect("doc.tree result should exist");
    assert_eq!(
        tree["total"].as_u64(),
        Some(0),
        "symlinked markdown outside workspace root must not be imported"
    );
}
