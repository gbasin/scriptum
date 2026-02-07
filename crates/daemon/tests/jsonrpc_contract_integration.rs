use std::{fs, path::PathBuf};

use scriptum_common::protocol::jsonrpc::{
    Request, RequestId, Response, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND,
};
use scriptum_daemon::rpc::methods::{handle_raw_request, GitState, RpcServerState};
use serde_json::{json, Value};
use uuid::Uuid;

const CONTRACT_METHODS: &[&str] = &[
    "rpc.ping",
    "daemon.shutdown",
    "doc.read",
    "doc.edit",
    "doc.bundle",
    "doc.edit_section",
    "doc.sections",
    "doc.diff",
    "doc.search",
    "doc.tree",
    "agent.whoami",
    "agent.status",
    "agent.conflicts",
    "agent.list",
    "agent.claim",
    "workspace.list",
    "workspace.open",
    "workspace.create",
    "git.status",
    "git.sync",
    "git.configure",
];

#[tokio::test]
async fn contract_methods_are_registered_in_dispatch() {
    let state = RpcServerState::default();

    for method in CONTRACT_METHODS {
        let request = Request::new(*method, None, RequestId::String((*method).to_string()));
        let response = call_raw(&state, &request).await;

        if let Some(error) = response.error {
            assert_ne!(
                error.code, METHOD_NOT_FOUND,
                "method `{method}` must exist in JSON-RPC dispatch contract",
            );
        }
    }
}

#[tokio::test]
async fn daemon_methods_accept_documented_param_shapes() {
    let state = contract_state_with_git();
    let workspace_id = Uuid::new_v4();
    let doc_id = Uuid::new_v4();
    let workspace_root = unique_temp_path("scriptum-jsonrpc-contract-workspace");
    fs::create_dir_all(&workspace_root).expect("workspace root path should be creatable");

    let cases: Vec<(&str, Option<Value>)> = vec![
        ("rpc.ping", Some(json!({}))),
        ("daemon.shutdown", Some(json!({}))),
        (
            "doc.read",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "include_content": true
            })),
        ),
        (
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": Uuid::new_v4().to_string(),
                "content_md": "# Contract test"
            })),
        ),
        (
            "doc.bundle",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "include": ["parents", "children", "comments"],
                "token_budget": 2048
            })),
        ),
        (
            "doc.edit_section",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "section": "intro",
                "content": "Updated section content",
                "agent": "contract-tester",
                "summary": "jsonrpc contract verification"
            })),
        ),
        (
            "doc.sections",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id
            })),
        ),
        (
            "doc.diff",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "from_seq": 0,
                "to_seq": 1
            })),
        ),
        (
            "doc.search",
            Some(json!({
                "workspace_id": workspace_id,
                "q": "contract",
                "limit": 10
            })),
        ),
        (
            "doc.tree",
            Some(json!({
                "workspace_id": workspace_id,
                "path_prefix": "docs/"
            })),
        ),
        ("agent.whoami", Some(json!({}))),
        (
            "agent.status",
            Some(json!({
                "workspace_id": workspace_id
            })),
        ),
        (
            "agent.conflicts",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id
            })),
        ),
        (
            "agent.list",
            Some(json!({
                "workspace_id": workspace_id
            })),
        ),
        (
            "agent.claim",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "section_id": "intro",
                "ttl_sec": 300,
                "mode": "shared",
                "note": "contract check",
                "agent_id": "mcp-agent"
            })),
        ),
        (
            "workspace.list",
            Some(json!({
                "offset": 0,
                "limit": 10
            })),
        ),
        (
            "workspace.open",
            Some(json!({
                "workspace_id": workspace_id
            })),
        ),
        (
            "workspace.create",
            Some(json!({
                "name": "Contract Workspace",
                "root_path": workspace_root
            })),
        ),
        ("git.status", Some(json!({}))),
        (
            "git.sync",
            Some(json!({
                "action": {
                    "commit": {
                        "message": "contract check"
                    }
                }
            })),
        ),
        (
            "git.configure",
            Some(json!({
                "policy": "manual"
            })),
        ),
    ];

    for (method, params) in cases {
        let request = Request::new(method, params, RequestId::String(method.to_string()));
        let response = call_raw(&state, &request).await;

        if let Some(error) = response.error {
            assert_ne!(
                error.code, INVALID_PARAMS,
                "method `{method}` rejected documented parameter shape",
            );
        }
    }
}

#[tokio::test]
async fn parameterized_methods_reject_non_object_params() {
    let state = contract_state_with_git();

    let methods = [
        "doc.read",
        "doc.edit",
        "doc.bundle",
        "doc.edit_section",
        "doc.sections",
        "doc.diff",
        "doc.search",
        "doc.tree",
        "agent.status",
        "agent.conflicts",
        "agent.list",
        "agent.claim",
        "workspace.list",
        "workspace.open",
        "workspace.create",
        "git.sync",
        "git.configure",
    ];

    for method in methods {
        let request = Request::new(
            method,
            Some(json!("not-an-object")),
            RequestId::String(format!("{method}-invalid")),
        );
        let response = call_raw(&state, &request).await;
        let error = response
            .error
            .unwrap_or_else(|| panic!("method `{method}` should reject non-object params"));
        assert_eq!(
            error.code, INVALID_PARAMS,
            "method `{method}` must reject non-object params with INVALID_PARAMS",
        );
    }
}

#[tokio::test]
async fn core_method_result_shapes_match_contract() {
    let state = contract_state_with_git();

    let ping = call_raw(
        &state,
        &Request::new("rpc.ping", Some(json!({})), RequestId::String("ping".to_string())),
    )
    .await;
    assert_eq!(ping.error, None);
    assert_eq!(ping.result, Some(json!({ "ok": true })));

    let whoami = call_raw(
        &state,
        &Request::new(
            "agent.whoami",
            Some(json!({})),
            RequestId::String("whoami".to_string()),
        ),
    )
    .await;
    assert_eq!(whoami.error, None);
    let whoami_result = whoami.result.expect("agent.whoami should return a result object");
    assert!(whoami_result.get("agent_id").is_some());
    assert!(
        whoami_result
            .get("capabilities")
            .and_then(|value| value.as_array())
            .is_some_and(|capabilities| !capabilities.is_empty()),
    );

    let workspace_root = unique_temp_path("scriptum-jsonrpc-shape-contract-workspace");
    fs::create_dir_all(&workspace_root).expect("workspace root path should be creatable");

    let create_workspace = call_raw(
        &state,
        &Request::new(
            "workspace.create",
            Some(json!({
                "name": "Shape Contract Workspace",
                "root_path": workspace_root,
            })),
            RequestId::String("workspace-create".to_string()),
        ),
    )
    .await;
    assert_eq!(create_workspace.error, None);

    let create_result =
        create_workspace.result.expect("workspace.create should return a result object");
    for key in ["workspace_id", "name", "root_path", "created_at"] {
        assert!(
            create_result.get(key).is_some(),
            "workspace.create result must contain `{key}`",
        );
    }

    let workspace_id: Uuid = serde_json::from_value(
        create_result
            .get("workspace_id")
            .cloned()
            .expect("workspace.create must return workspace_id"),
    )
    .expect("workspace_id should be a valid uuid");

    let list_workspaces = call_raw(
        &state,
        &Request::new(
            "workspace.list",
            Some(json!({ "offset": 0, "limit": 10 })),
            RequestId::String("workspace-list".to_string()),
        ),
    )
    .await;
    assert_eq!(list_workspaces.error, None);
    let list_result = list_workspaces.result.expect("workspace.list should return a result object");
    assert!(list_result.get("items").is_some());
    assert!(list_result.get("total").is_some());

    let open_workspace = call_raw(
        &state,
        &Request::new(
            "workspace.open",
            Some(json!({ "workspace_id": workspace_id })),
            RequestId::String("workspace-open".to_string()),
        ),
    )
    .await;
    assert_eq!(open_workspace.error, None);
    let open_result = open_workspace.result.expect("workspace.open should return a result object");
    for key in ["workspace_id", "name", "root_path", "doc_count", "created_at"] {
        assert!(
            open_result.get(key).is_some(),
            "workspace.open result must contain `{key}`",
        );
    }
}

#[tokio::test]
async fn jsonrpc_version_matrix_supports_current_and_rejects_legacy() {
    let state = RpcServerState::default();

    for version in ["2.0"] {
        let response = call_raw_json(
            &state,
            json!({
                "jsonrpc": version,
                "id": format!("supported-{version}"),
                "method": "rpc.ping",
                "params": {}
            }),
        )
        .await;

        if let Some(error) = response.error {
            assert_ne!(
                error.code, INVALID_REQUEST,
                "json-rpc version `{version}` should be accepted",
            );
        }
    }

    for version in ["1.0", "1.1", "2.1"] {
        let response = call_raw_json(
            &state,
            json!({
                "jsonrpc": version,
                "id": format!("unsupported-{version}"),
                "method": "rpc.ping",
                "params": {}
            }),
        )
        .await;

        let error = response.error.expect("unsupported version should return an error");
        assert_eq!(
            error.code, INVALID_REQUEST,
            "json-rpc version `{version}` should be rejected",
        );
    }
}

async fn call_raw(state: &RpcServerState, request: &Request) -> Response {
    let raw = serde_json::to_vec(request).expect("request should serialize");
    handle_raw_request(&raw, state).await
}

async fn call_raw_json(state: &RpcServerState, payload: serde_json::Value) -> Response {
    let raw = serde_json::to_vec(&payload).expect("payload should serialize");
    handle_raw_request(&raw, state).await
}

fn contract_state_with_git() -> RpcServerState {
    let git_root = unique_temp_path("scriptum-jsonrpc-contract-git");
    fs::create_dir_all(&git_root).expect("git root path should be creatable");
    RpcServerState::default().with_git_state(GitState::new(git_root))
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()))
}
