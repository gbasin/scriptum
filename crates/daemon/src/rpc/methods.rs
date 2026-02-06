use std::collections::HashMap;
use std::sync::Arc;

use crate::engine::{doc_manager::DocManager, ydoc::YDoc};
use scriptum_common::protocol::jsonrpc::{
    Request, RequestId, Response, RpcError, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST,
    METHOD_NOT_FOUND, PARSE_ERROR,
};
use scriptum_common::section::parser::parse_sections;
use scriptum_common::types::Section;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone)]
pub struct RpcServerState {
    doc_manager: Arc<RwLock<DocManager>>,
    doc_metadata: Arc<RwLock<HashMap<(Uuid, Uuid), DocMetadataRecord>>>,
    shutdown_notifier: Option<broadcast::Sender<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocMetadataRecord {
    pub workspace_id: Uuid,
    pub doc_id: Uuid,
    pub path: String,
    pub title: String,
    pub head_seq: i64,
    pub etag: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DocReadParams {
    workspace_id: Uuid,
    doc_id: Uuid,
    #[serde(default)]
    include_content: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DocReadResult {
    metadata: DocMetadataRecord,
    sections: Vec<Section>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_md: Option<String>,
}

impl Default for RpcServerState {
    fn default() -> Self {
        Self {
            doc_manager: Arc::new(RwLock::new(DocManager::default())),
            doc_metadata: Arc::new(RwLock::new(HashMap::new())),
            shutdown_notifier: None,
        }
    }
}

impl RpcServerState {
    pub fn with_shutdown_notifier(mut self, shutdown_notifier: broadcast::Sender<()>) -> Self {
        self.shutdown_notifier = Some(shutdown_notifier);
        self
    }

    pub async fn seed_doc(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        path: impl Into<String>,
        title: impl Into<String>,
        markdown: impl AsRef<str>,
    ) {
        let markdown = markdown.as_ref();
        let doc = YDoc::new();
        if !markdown.is_empty() {
            doc.insert_text("content", 0, markdown);
        }

        {
            let mut manager = self.doc_manager.write().await;
            manager.put_doc(doc_id, doc);
        }

        let path = path.into();
        let title = title.into();
        let metadata = DocMetadataRecord {
            workspace_id,
            doc_id,
            path,
            title,
            head_seq: 0,
            etag: format!("doc:{doc_id}:0"),
        };
        self.doc_metadata.write().await.insert((workspace_id, doc_id), metadata);
    }

    async fn read_doc(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        include_content: bool,
    ) -> DocReadResult {
        let doc = {
            let mut manager = self.doc_manager.write().await;
            manager.subscribe_or_create(doc_id)
        };

        let metadata = {
            let mut metadata = self.doc_metadata.write().await;
            metadata
                .entry((workspace_id, doc_id))
                .or_insert_with(|| default_metadata(workspace_id, doc_id))
                .clone()
        };

        let content_md = doc.get_text_string("content");
        let sections = parse_sections(&content_md);

        {
            let mut manager = self.doc_manager.write().await;
            let _ = manager.unsubscribe(doc_id);
        }

        DocReadResult { metadata, sections, content_md: include_content.then_some(content_md) }
    }
}

pub async fn handle_raw_request(raw: &[u8], state: &RpcServerState) -> Response {
    let request = match serde_json::from_slice::<Request>(raw) {
        Ok(request) => request,
        Err(error) => {
            return Response::error(
                RequestId::Null,
                RpcError {
                    code: PARSE_ERROR,
                    message: "Parse error".to_string(),
                    data: Some(json!({ "reason": error.to_string() })),
                },
            );
        }
    };

    if request.jsonrpc != "2.0" {
        return Response::error(
            request.id,
            RpcError { code: INVALID_REQUEST, message: "Invalid Request".to_string(), data: None },
        );
    }

    dispatch_request(request, state).await
}

pub async fn dispatch_request(request: Request, state: &RpcServerState) -> Response {
    match request.method.as_str() {
        "rpc.ping" => Response::success(
            request.id,
            json!({
                "ok": true,
            }),
        ),
        "daemon.shutdown" => {
            if let Some(notifier) = &state.shutdown_notifier {
                let _ = notifier.send(());
            }
            Response::success(
                request.id,
                json!({
                    "ok": true,
                }),
            )
        }
        "doc.read" => handle_doc_read(request, state).await,
        "rpc.internal_error" => Response::error(
            request.id,
            RpcError { code: INTERNAL_ERROR, message: "Internal error".to_string(), data: None },
        ),
        _ => Response::error(
            request.id,
            RpcError {
                code: METHOD_NOT_FOUND,
                message: "Method not found".to_string(),
                data: None,
            },
        ),
    }
}

async fn handle_doc_read(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_doc_read_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    let result = state.read_doc(params.workspace_id, params.doc_id, params.include_content).await;
    Response::success(request.id, json!(result))
}

fn parse_doc_read_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<DocReadParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "doc.read requires params".to_string()));
    };

    serde_json::from_value::<DocReadParams>(params).map_err(|error| {
        invalid_params_response(request_id, format!("failed to decode doc.read params: {}", error))
    })
}

fn invalid_params_response(request_id: RequestId, reason: String) -> Response {
    Response::error(
        request_id,
        RpcError {
            code: INVALID_PARAMS,
            message: "Invalid params".to_string(),
            data: Some(json!({ "reason": reason })),
        },
    )
}

fn default_metadata(workspace_id: Uuid, doc_id: Uuid) -> DocMetadataRecord {
    DocMetadataRecord {
        workspace_id,
        doc_id,
        path: format!("{doc_id}.md"),
        title: "Untitled".to_string(),
        head_seq: 0,
        etag: format!("doc:{doc_id}:0"),
    }
}

#[cfg(test)]
mod tests {
    use scriptum_common::protocol::jsonrpc::{Request, RequestId};
    use serde_json::json;
    use tokio::sync::broadcast;
    use uuid::Uuid;

    use super::{dispatch_request, RpcServerState};

    #[tokio::test]
    async fn doc_read_returns_content_and_sections() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let markdown = "# Root\n\n## Child\n";
        state.seed_doc(workspace_id, doc_id, "docs/readme.md", "Readme", markdown).await;

        let request = Request::new(
            "doc.read",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "include_content": true
            })),
            RequestId::Number(1),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["metadata"]["workspace_id"], json!(workspace_id));
        assert_eq!(result["metadata"]["doc_id"], json!(doc_id));
        assert_eq!(result["metadata"]["path"], json!("docs/readme.md"));
        assert_eq!(result["metadata"]["title"], json!("Readme"));
        assert_eq!(result["content_md"], json!(markdown));
        assert_eq!(result["sections"].as_array().expect("sections should be an array").len(), 2);
    }

    #[tokio::test]
    async fn doc_read_omits_content_when_include_content_is_false() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/note.md", "Note", "# Heading\n\nBody").await;

        let request = Request::new(
            "doc.read",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "include_content": false
            })),
            RequestId::Number(2),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result.get("content_md"), None);
        assert_eq!(result["sections"].as_array().expect("sections should be an array").len(), 1);
    }

    #[tokio::test]
    async fn doc_read_rejects_invalid_params() {
        let state = RpcServerState::default();
        let request = Request::new(
            "doc.read",
            Some(json!({
                "workspace_id": Uuid::new_v4(),
                // missing doc_id
                "include_content": true
            })),
            RequestId::Number(3),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.result.is_none());
        let error = response.error.expect("error should be present");
        assert_eq!(error.code, -32602);
    }

    #[tokio::test]
    async fn daemon_shutdown_notifies_runtime_when_configured() {
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
        let state = RpcServerState::default().with_shutdown_notifier(shutdown_tx);
        let request = Request::new("daemon.shutdown", None, RequestId::Number(4));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        assert_eq!(response.result.expect("result should be populated"), json!({ "ok": true }));
        shutdown_rx.recv().await.expect("shutdown notification should be sent");
    }
}
