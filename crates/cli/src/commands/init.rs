// `scriptum init` — initialize a Scriptum workspace.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::DaemonClient;
use crate::daemon_launcher;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Workspace path (defaults to current directory).
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,

    /// Initialize a git repository if one does not exist.
    #[arg(long)]
    git: bool,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitResult {
    pub workspace_id: String,
    pub name: String,
    pub root_path: String,
    #[serde(default)]
    pub git_initialized: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceCreateRpcResult {
    workspace_id: String,
    name: String,
    root_path: String,
}

pub fn run(args: InitArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let root_path = resolve_workspace_root(args.path)?;

    if let Err(error) = ensure_workspace_not_initialized(&root_path) {
        output::print_error(format, "WORKSPACE_ALREADY_INITIALIZED", &error.to_string());
        return Err(error);
    }

    let workspace_name = workspace_name_from_path(&root_path);
    let init = tokio::runtime::Handle::try_current()
        .map(|handle| {
            handle.block_on(run_init(root_path.clone(), workspace_name.clone(), args.git))
        })
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(run_init(root_path, workspace_name, args.git))
        });

    match init {
        Ok(result) => {
            output::print_output(format, &result, format_human)?;
            Ok(())
        }
        Err(error) => {
            output::print_error(format, "INIT_ERROR", &format!("{error:#}"));
            Err(error)
        }
    }
}

async fn run_init(
    root_path: PathBuf,
    workspace_name: String,
    git: bool,
) -> anyhow::Result<InitResult> {
    daemon_launcher::ensure_daemon_running().await?;
    let client = DaemonClient::default();
    initialize_workspace_with_client(&client, &root_path, &workspace_name, git).await
}

async fn initialize_workspace_with_client(
    client: &DaemonClient,
    root_path: &Path,
    workspace_name: &str,
    git: bool,
) -> anyhow::Result<InitResult> {
    let root_path_text = root_path.to_string_lossy().to_string();
    let rpc_result: WorkspaceCreateRpcResult = client
        .call(
            "workspace.create",
            json!({
                "name": workspace_name,
                "root_path": root_path_text,
            }),
        )
        .await
        .context("workspace.create request failed")?;

    let git_initialized = if git { ensure_git_repository(root_path)? } else { false };

    Ok(InitResult {
        workspace_id: rpc_result.workspace_id,
        name: rpc_result.name,
        root_path: rpc_result.root_path,
        git_initialized,
    })
}

fn ensure_workspace_not_initialized(root_path: &Path) -> anyhow::Result<()> {
    if root_path.join(".scriptum").exists() {
        bail!("workspace already initialized at `{}`", root_path.to_string_lossy());
    }
    Ok(())
}

fn resolve_workspace_root(path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let provided = path.unwrap_or_else(|| PathBuf::from("."));
    if provided.is_absolute() {
        return Ok(provided);
    }

    std::env::current_dir()
        .map(|cwd| cwd.join(provided))
        .context("failed to resolve current working directory")
}

fn workspace_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

fn is_git_repository(path: &Path) -> anyhow::Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("failed to run `git rev-parse --is-inside-work-tree`")?;
    Ok(output.status.success())
}

fn ensure_git_repository(path: &Path) -> anyhow::Result<bool> {
    std::fs::create_dir_all(path).with_context(|| {
        format!(
            "failed to create workspace directory before git init at `{}`",
            path.to_string_lossy()
        )
    })?;

    if is_git_repository(path)? {
        return Ok(false);
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("init")
        .output()
        .context("failed to run `git init`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git init failed at `{}`: {}", path.to_string_lossy(), stderr.trim());
    }

    Ok(true)
}

fn format_human(result: &InitResult) -> String {
    let mut lines = vec![format!("Initialized Scriptum workspace at {}", result.root_path)];
    if result.git_initialized {
        lines.push("Initialized git repository.".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    #[cfg(unix)]
    use tokio::net::UnixListener;

    #[test]
    fn workspace_name_uses_directory_name() {
        let path = PathBuf::from("/tmp/my-project");
        assert_eq!(workspace_name_from_path(&path), "my-project");
    }

    #[test]
    fn workspace_name_falls_back_for_root_like_paths() {
        assert_eq!(workspace_name_from_path(Path::new("/")), "workspace");
    }

    #[test]
    fn format_human_includes_git_line_when_initialized() {
        let result = InitResult {
            workspace_id: "ws-123".to_string(),
            name: "Project".to_string(),
            root_path: "/tmp/project".to_string(),
            git_initialized: true,
        };

        let output = format_human(&result);
        assert!(output.contains("Initialized Scriptum workspace"));
        assert!(output.contains("Initialized git repository."));
    }

    #[test]
    fn format_human_omits_git_line_when_not_requested() {
        let result = InitResult {
            workspace_id: "ws-123".to_string(),
            name: "Project".to_string(),
            root_path: "/tmp/project".to_string(),
            git_initialized: false,
        };

        let output = format_human(&result);
        assert!(output.contains("Initialized Scriptum workspace"));
        assert!(!output.contains("Initialized git repository."));
    }

    #[test]
    fn detects_existing_workspace_marker() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let root = tmp.path().join("existing");
        std::fs::create_dir_all(root.join(".scriptum"))
            .expect("existing workspace marker should be created");

        let error = ensure_workspace_not_initialized(&root)
            .expect_err("workspace marker should fail init preflight");
        assert!(error.to_string().contains("workspace already initialized"));
    }

    #[test]
    fn git_repository_helper_initializes_only_once() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let root = tmp.path().join("git-workspace");
        std::fs::create_dir_all(&root).expect("workspace root should be created");

        let first = match ensure_git_repository(&root) {
            Ok(initialized) => initialized,
            Err(error) if error.to_string().contains("failed to run `git") => {
                eprintln!("skipping git helper test: git not available");
                return;
            }
            Err(error) => panic!("git repository should initialize: {error:#}"),
        };
        assert!(first, "first initialization should run git init");
        assert!(root.join(".git").exists(), "git metadata directory should exist");

        let second = ensure_git_repository(&root)
            .expect("re-initializing an existing repository should succeed");
        assert!(!second, "second initialization should be a no-op");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn init_then_workspace_list_reports_empty_workspace() {
        #[derive(Debug, Deserialize)]
        struct WorkspaceListResult {
            items: Vec<WorkspaceListItem>,
            total: usize,
        }

        #[derive(Debug, Deserialize)]
        struct WorkspaceListItem {
            workspace_id: String,
            doc_count: usize,
        }

        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let socket_path = tmp.path().join("daemon.sock");
        let listener =
            UnixListener::bind(&socket_path).expect("unix listener should bind for mock daemon");

        let workspace_root = tmp.path().join("new-workspace");
        let workspace_root_str = workspace_root.to_string_lossy().to_string();
        let workspace_name = workspace_name_from_path(&workspace_root);
        let workspace_id = "00000000-0000-0000-0000-000000000123".to_string();

        let workspace_root_for_server = workspace_root_str.clone();
        let workspace_name_for_server = workspace_name.clone();
        let workspace_id_for_server = workspace_id.clone();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.expect("accept should succeed");
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut request_line = Vec::new();
                reader
                    .read_until(b'\n', &mut request_line)
                    .await
                    .expect("request line should be readable");

                let request: serde_json::Value =
                    serde_json::from_slice(&request_line).expect("request should parse as json");
                let method = request["method"].as_str().unwrap_or_default();
                let id = request["id"].clone();

                let response = match method {
                    "workspace.create" => {
                        assert_eq!(request["params"]["name"], workspace_name_for_server);
                        assert_eq!(request["params"]["root_path"], workspace_root_for_server);
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "workspace_id": workspace_id_for_server,
                                "name": workspace_name_for_server,
                                "root_path": workspace_root_for_server,
                            }
                        })
                    }
                    "workspace.list" => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "items": [
                                {
                                    "workspace_id": workspace_id_for_server,
                                    "doc_count": 0
                                }
                            ],
                            "next_cursor": null,
                            "total": 1
                        }
                    }),
                    other => {
                        panic!("unexpected rpc method in init integration test: {other}");
                    }
                };

                let payload =
                    serde_json::to_string(&response).expect("response should serialize") + "\n";
                write_half
                    .write_all(payload.as_bytes())
                    .await
                    .expect("response should write to socket");
            }
        });

        let client = DaemonClient::new(socket_path.clone());
        let init_result =
            initialize_workspace_with_client(&client, &workspace_root, &workspace_name, false)
                .await
                .expect("workspace initialization should succeed");
        assert_eq!(init_result.workspace_id, workspace_id);
        assert_eq!(init_result.root_path, workspace_root_str);

        let list: WorkspaceListResult = client
            .call("workspace.list", json!({ "offset": 0, "limit": 20 }))
            .await
            .expect("workspace.list should succeed after init");
        assert_eq!(list.total, 1);
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].workspace_id, workspace_id);
        assert_eq!(list.items[0].doc_count, 0);

        server.await.expect("server task should complete");
        let _ = std::fs::remove_file(socket_path);
    }
}
