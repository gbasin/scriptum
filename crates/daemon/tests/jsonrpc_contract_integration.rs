use scriptum_common::protocol::jsonrpc::{
    Request, RequestId, Response, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND,
};
use scriptum_daemon::rpc::methods::{handle_raw_request, RpcServerState};
use serde_json::json;
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
async fn mcp_passthrough_methods_accept_documented_param_shapes() {
    let state = RpcServerState::default();
    let workspace_id = Uuid::new_v4();
    let doc_id = Uuid::new_v4();
    let client_update_id = Uuid::new_v4().to_string();

    let cases = [
        (
            "doc.read",
            json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "include_content": true
            }),
        ),
        (
            "doc.edit",
            json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": client_update_id,
                "content_md": "# Contract test"
            }),
        ),
        (
            "doc.tree",
            json!({
                "workspace_id": workspace_id
            }),
        ),
        (
            "doc.sections",
            json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id
            }),
        ),
    ];

    for (method, params) in cases {
        let request = Request::new(method, Some(params), RequestId::String(method.to_string()));
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
