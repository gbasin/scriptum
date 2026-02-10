// `scriptum new` — create a new markdown document through daemon RPC.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct NewArgs {
    /// Relative markdown path to create (e.g. `docs/api-spec.md`).
    pub path: String,

    /// Optional document title metadata.
    #[arg(long)]
    title: Option<String>,

    /// Optional template file path for initial markdown content.
    #[arg(long, value_name = "FILE")]
    template: Option<PathBuf>,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewResult {
    pub document: NewDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDocument {
    pub id: String,
    pub workspace_id: String,
    pub path: String,
    pub title: String,
    pub head_seq: i64,
    pub etag: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceOpenRpcResult {
    workspace_id: String,
}

#[derive(Debug, Clone)]
struct NewRequest {
    path: String,
    title: Option<String>,
    initial_content: Option<String>,
}

pub fn run(args: NewArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let workspace_root = detect_workspace_root_from_cwd()?;
    let request = NewRequest {
        path: args.path,
        title: args.title,
        initial_content: load_template_content(args.template.as_deref())?,
    };

    let rt = tokio::runtime::Handle::try_current()
        .map(|handle| handle.block_on(call_new(workspace_root.clone(), request.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_new(workspace_root, request))
        });

    match rt {
        Ok(result) => {
            output::print_output(format, &result, format_human)?;
            Ok(())
        }
        Err(error) => {
            output::print_anyhow_error(format, &error);
            Err(error)
        }
    }
}

async fn call_new(workspace_root: PathBuf, request: NewRequest) -> anyhow::Result<NewResult> {
    let client = DaemonClient::default();
    create_document_with_client(&client, &workspace_root, &request).await
}

async fn create_document_with_client(
    client: &DaemonClient,
    workspace_root: &Path,
    request: &NewRequest,
) -> anyhow::Result<NewResult> {
    let workspace_root_text = workspace_root.to_string_lossy().to_string();
    let workspace: WorkspaceOpenRpcResult = client
        .call(
            rpc_methods::WORKSPACE_OPEN,
            json!({
                "root_path": workspace_root_text,
            }),
        )
        .await
        .context("workspace.open request failed")?;

    let mut params = json!({
        "workspace_id": workspace.workspace_id,
        "path": request.path,
    });
    if let Some(title) = &request.title {
        params["title"] = json!(title);
    }
    if let Some(initial_content) = &request.initial_content {
        params["initial_content"] = json!(initial_content);
    }

    client.call(rpc_methods::DOC_CREATE, params).await.context("doc.create request failed")
}

fn detect_workspace_root_from_cwd() -> anyhow::Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    find_workspace_root(&cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "no Scriptum workspace found from `{}`; run `scriptum init` first",
            cwd.display()
        )
    })
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|candidate| {
        let marker = candidate.join(".scriptum").join("workspace.toml");
        if marker.is_file() {
            Some(candidate.to_path_buf())
        } else {
            None
        }
    })
}

fn load_template_content(template_path: Option<&Path>) -> anyhow::Result<Option<String>> {
    let Some(path) = template_path else {
        return Ok(None);
    };
    if path.as_os_str().is_empty() {
        bail!("template path must not be empty");
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read template file `{}`", path.display()))?;
    Ok(Some(content))
}

fn format_human(result: &NewResult) -> String {
    format!(
        "Created {} (title: {}, etag: {})",
        result.document.path, result.document.title, result.document.etag
    )
}

#[cfg(test)]
mod tests {
    use std::io;

    #[cfg(unix)]
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    #[cfg(unix)]
    use tokio::net::UnixListener;

    use super::*;

    fn sample_result() -> NewResult {
        NewResult {
            document: NewDocument {
                id: "4f6f98c8-dce7-4d00-8e31-35eb8f744b43".to_string(),
                workspace_id: "d9032f4d-1d55-42e9-a5f9-87f9696525bb".to_string(),
                path: "docs/api-spec.md".to_string(),
                title: "API Spec".to_string(),
                head_seq: 0,
                etag: "doc:4f6f98c8-dce7-4d00-8e31-35eb8f744b43:0".to_string(),
            },
        }
    }

    #[test]
    fn human_format_shows_created_document() {
        let output = format_human(&sample_result());
        assert!(output.contains("Created docs/api-spec.md"));
        assert!(output.contains("title: API Spec"));
        assert!(output.contains("etag: doc:4f6f98c8-dce7-4d00-8e31-35eb8f744b43:0"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: NewResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.document.path, "docs/api-spec.md");
        assert_eq!(parsed.document.title, "API Spec");
        assert_eq!(parsed.document.head_seq, 0);
    }

    #[test]
    fn find_workspace_root_walks_ancestor_chain() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let workspace_root = tmp.path().join("workspace");
        let nested = workspace_root.join("docs").join("spec");
        std::fs::create_dir_all(&nested).expect("nested directory should be created");
        std::fs::create_dir_all(workspace_root.join(".scriptum"))
            .expect("workspace marker directory should be created");
        std::fs::write(workspace_root.join(".scriptum").join("workspace.toml"), "workspace = true")
            .expect("workspace marker file should be created");

        let detected = find_workspace_root(&nested).expect("workspace root should be detected");
        assert_eq!(detected, workspace_root);
    }

    #[test]
    fn find_workspace_root_returns_none_without_marker() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let nested = tmp.path().join("no-workspace").join("docs");
        std::fs::create_dir_all(&nested).expect("nested directory should be created");
        assert!(find_workspace_root(&nested).is_none());
    }

    #[test]
    fn load_template_content_reads_file() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let template = tmp.path().join("template.md");
        std::fs::write(&template, "# Seed\n\nTemplate body.")
            .expect("template file should be writable");

        let loaded = load_template_content(Some(&template)).expect("template content should load");
        assert_eq!(loaded.as_deref(), Some("# Seed\n\nTemplate body."));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn create_document_calls_workspace_open_then_doc_create() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let socket_path = tmp.path().join("daemon.sock");
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("skipping unix socket integration test: {error}");
                return;
            }
            Err(error) => panic!("unix listener should bind: {error}"),
        };

        let workspace_root = tmp.path().join("workspace");
        std::fs::create_dir_all(workspace_root.join(".scriptum"))
            .expect("workspace marker directory should be created");
        std::fs::write(workspace_root.join(".scriptum").join("workspace.toml"), "workspace = true")
            .expect("workspace marker file should be written");
        let workspace_root_text = workspace_root.to_string_lossy().to_string();
        let workspace_id = "8fcf3187-ac85-44f6-b4f7-8e010ad6ac15".to_string();

        let expected_request = NewRequest {
            path: "docs/api-spec.md".to_string(),
            title: Some("API Spec".to_string()),
            initial_content: Some("# API Spec\n\nTemplate content.".to_string()),
        };
        let expected_request_for_server = expected_request.clone();
        let workspace_root_for_server = workspace_root_text.clone();
        let workspace_id_for_server = workspace_id.clone();
        let server = tokio::spawn(async move {
            for turn in 0..2 {
                let (stream, _) = listener.accept().await.expect("accept should succeed");
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut request_line = Vec::new();
                reader
                    .read_until(b'\n', &mut request_line)
                    .await
                    .expect("request should be readable");
                let request: serde_json::Value =
                    serde_json::from_slice(&request_line).expect("request should decode as JSON");

                let response = if turn == 0 {
                    assert_eq!(request["method"], rpc_methods::WORKSPACE_OPEN);
                    assert_eq!(
                        request["params"]["root_path"],
                        serde_json::Value::String(workspace_root_for_server.clone())
                    );
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {
                            "workspace_id": workspace_id_for_server,
                        }
                    })
                } else {
                    assert_eq!(request["method"], rpc_methods::DOC_CREATE);
                    assert_eq!(
                        request["params"]["workspace_id"],
                        serde_json::Value::String(workspace_id_for_server.clone())
                    );
                    assert_eq!(
                        request["params"]["path"],
                        serde_json::Value::String(expected_request_for_server.path.clone())
                    );
                    assert_eq!(
                        request["params"]["title"],
                        serde_json::Value::String(
                            expected_request_for_server
                                .title
                                .as_ref()
                                .expect("expected title should exist")
                                .clone()
                        )
                    );
                    assert_eq!(
                        request["params"]["initial_content"],
                        serde_json::Value::String(
                            expected_request_for_server
                                .initial_content
                                .as_ref()
                                .expect("expected content should exist")
                                .clone()
                        )
                    );
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {
                            "document": {
                                "id": "4f6f98c8-dce7-4d00-8e31-35eb8f744b43",
                                "workspace_id": workspace_id_for_server,
                                "path": "docs/api-spec.md",
                                "title": "API Spec",
                                "head_seq": 0,
                                "etag": "doc:4f6f98c8-dce7-4d00-8e31-35eb8f744b43:0"
                            }
                        }
                    })
                };

                let payload = response.to_string() + "\n";
                write_half
                    .write_all(payload.as_bytes())
                    .await
                    .expect("response write should succeed");
            }
        });

        let client = DaemonClient::new(socket_path.clone());
        let created = create_document_with_client(&client, &workspace_root, &expected_request)
            .await
            .expect("create document should succeed");
        assert_eq!(created.document.path, "docs/api-spec.md");
        assert_eq!(created.document.title, "API Spec");
        assert_eq!(created.document.workspace_id, workspace_id);

        server.await.expect("server should complete");
    }
}
