use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use scriptum_common::protocol::jsonrpc::{Request, RequestId, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::UnixStream;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::rpc::methods::RpcServerState;
use crate::rpc::unix::serve_unix_until_shutdown;
use crate::rpc::yjs_ws::{self, YjsWsState};
use crate::startup::{
    bind_socket, is_daemon_running, remove_pid_file, write_pid_file, DaemonPaths,
};

const TAKEOVER_WAIT_RETRIES: usize = 40;
const TAKEOVER_WAIT_DELAY: Duration = Duration::from_millis(50);
const LOCAL_YJS_WS_ADDR: &str = "127.0.0.1:39091";

pub struct EmbeddedDaemonHandle {
    shutdown_tx: broadcast::Sender<()>,
    task: Option<JoinHandle<()>>,
}

impl EmbeddedDaemonHandle {
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    pub async fn wait(mut self) {
        self.shutdown();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for EmbeddedDaemonHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub async fn start_embedded() -> Result<EmbeddedDaemonHandle> {
    start_embedded_with_paths(DaemonPaths::resolve()?).await
}

pub async fn run_standalone() -> Result<()> {
    run_standalone_with_paths(DaemonPaths::resolve()?).await
}

async fn run_standalone_with_paths(paths: DaemonPaths) -> Result<()> {
    let listener = bind_socket(&paths.socket_path).await?;
    write_pid_file(&paths.pid_path)?;
    let yjs_ws_task = start_local_yjs_ws_server().await?;

    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);
    let state = RpcServerState::default()
        .with_crdt_store_dir(paths.base_dir.join("crdt_store"))
        .with_shutdown_notifier(shutdown_tx.clone());
    recover_state_from_crdt_store(&state, &paths.base_dir).await?;
    let ctrl_c_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = ctrl_c_tx.send(());
    });

    info!(socket_path = %paths.socket_path.display(), "standalone daemon started");
    let result = serve_unix_until_shutdown(listener, state, shutdown_rx).await;
    yjs_ws_task.abort();
    let _ = yjs_ws_task.await;
    cleanup_paths(&paths);
    result.context("standalone daemon exited with error")
}

async fn start_embedded_with_paths(paths: DaemonPaths) -> Result<EmbeddedDaemonHandle> {
    take_over_standalone_if_running(&paths.socket_path).await?;

    let listener = bind_socket(&paths.socket_path).await?;
    write_pid_file(&paths.pid_path)?;
    let yjs_ws_task = start_local_yjs_ws_server().await?;

    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);
    let state = RpcServerState::default()
        .with_crdt_store_dir(paths.base_dir.join("crdt_store"))
        .with_shutdown_notifier(shutdown_tx.clone());
    recover_state_from_crdt_store(&state, &paths.base_dir).await?;
    let socket_path = paths.socket_path.clone();
    let pid_path = paths.pid_path.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = serve_unix_until_shutdown(listener, state, shutdown_rx).await {
            warn!(?error, "embedded daemon server terminated unexpectedly");
        }
        yjs_ws_task.abort();
        let _ = yjs_ws_task.await;
        remove_pid_file(&pid_path);
        let _ = std::fs::remove_file(&socket_path);
    });

    info!(socket_path = %paths.socket_path.display(), "embedded daemon started");
    Ok(EmbeddedDaemonHandle { shutdown_tx, task: Some(task) })
}

async fn take_over_standalone_if_running(socket_path: &Path) -> Result<()> {
    if !is_daemon_running(socket_path).await {
        return Ok(());
    }

    info!(socket_path = %socket_path.display(), "standalone daemon detected, requesting shutdown");
    request_daemon_shutdown(socket_path).await?;
    wait_for_daemon_shutdown(socket_path).await
}

async fn request_daemon_shutdown(socket_path: &Path) -> Result<()> {
    let request = Request::new("daemon.shutdown", None, RequestId::Number(1));
    let encoded =
        serde_json::to_vec(&request).context("failed to serialize daemon shutdown request")?;
    let mut stream =
        UnixStream::connect(socket_path).await.context("failed to connect to running daemon")?;
    stream.write_all(&encoded).await.context("failed to send daemon shutdown request")?;
    stream.write_all(b"\n").await.context("failed to send daemon shutdown request terminator")?;
    stream.flush().await.context("failed to flush daemon shutdown request")?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let bytes_read =
        reader.read_line(&mut line).await.context("failed to read daemon shutdown response")?;
    if bytes_read == 0 {
        return Ok(());
    }

    let response: Response =
        serde_json::from_str(line.trim()).context("failed to decode daemon shutdown response")?;
    if let Some(error) = response.error {
        return Err(anyhow!("daemon refused shutdown request: {}", error.message));
    }

    Ok(())
}

async fn wait_for_daemon_shutdown(socket_path: &Path) -> Result<()> {
    for _ in 0..TAKEOVER_WAIT_RETRIES {
        if !is_daemon_running(socket_path).await {
            return Ok(());
        }
        tokio::time::sleep(TAKEOVER_WAIT_DELAY).await;
    }

    Err(anyhow!(
        "standalone daemon did not exit after takeover request at `{}`",
        socket_path.display()
    ))
}

fn cleanup_paths(paths: &DaemonPaths) {
    remove_pid_file(&paths.pid_path);
    let _ = std::fs::remove_file(&paths.socket_path);
}

async fn recover_state_from_crdt_store(state: &RpcServerState, base_dir: &Path) -> Result<()> {
    let crdt_store_dir = base_dir.join("crdt_store");
    let report = state
        .recover_docs_at_startup(&crdt_store_dir)
        .await
        .map_err(|error| anyhow!("startup crash recovery failed: {error}"))?;
    if report.degraded_docs.is_empty() {
        info!(
            recovered_docs = report.recovered_docs,
            path = %crdt_store_dir.display(),
            "startup crash recovery completed"
        );
    } else {
        warn!(
            recovered_docs = report.recovered_docs,
            degraded_docs = report.degraded_docs.len(),
            degraded_doc_ids = ?report.degraded_docs,
            path = %crdt_store_dir.display(),
            "startup crash recovery completed with degraded documents"
        );
    }
    Ok(())
}

async fn start_local_yjs_ws_server() -> Result<JoinHandle<()>> {
    let listener = TcpListener::bind(LOCAL_YJS_WS_ADDR).await.with_context(|| {
        format!("failed to bind local yjs websocket server at `{LOCAL_YJS_WS_ADDR}`")
    })?;
    info!(address = LOCAL_YJS_WS_ADDR, "local yjs websocket server started");

    Ok(tokio::spawn(async move {
        if let Err(error) = yjs_ws::serve(listener, YjsWsState::default()).await {
            warn!(?error, "local yjs websocket server terminated unexpectedly");
        }
    }))
}

#[cfg(test)]
mod contract_tests {
    #[test]
    fn daemon_bind_address_matches_contract() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../contracts/daemon-ports.json");
        let content = std::fs::read_to_string(path).expect("contract file should be readable");
        let contract: serde_json::Value =
            serde_json::from_str(&content).expect("contract file should be valid JSON");

        let host = contract["host"].as_str().expect("host should be a string");
        let port = contract["port"].as_u64().expect("port should be a number");
        let expected_addr = format!("{host}:{port}");

        assert_eq!(super::LOCAL_YJS_WS_ADDR, expected_addr);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::{run_standalone_with_paths, start_embedded_with_paths, DaemonPaths};

    fn temp_paths(tmp: &TempDir) -> DaemonPaths {
        let base_dir = tmp.path().to_path_buf();
        DaemonPaths {
            base_dir: base_dir.clone(),
            socket_path: base_dir.join("daemon.sock"),
            pid_path: base_dir.join("daemon.pid"),
        }
    }

    #[tokio::test]
    async fn embedded_startup_takes_over_running_standalone_daemon() {
        let tmp = TempDir::new().expect("temp dir should be created");
        let paths = temp_paths(&tmp);

        let standalone_paths = DaemonPaths {
            base_dir: PathBuf::from(&paths.base_dir),
            socket_path: PathBuf::from(&paths.socket_path),
            pid_path: PathBuf::from(&paths.pid_path),
        };
        let standalone =
            tokio::spawn(async move { run_standalone_with_paths(standalone_paths).await });

        for _ in 0..40 {
            if super::is_daemon_running(&paths.socket_path).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            super::is_daemon_running(&paths.socket_path).await,
            "standalone daemon should be accepting connections before takeover test"
        );

        let embedded = start_embedded_with_paths(paths)
            .await
            .expect("embedded daemon should start and take over");

        let standalone_result = tokio::time::timeout(Duration::from_secs(5), standalone)
            .await
            .expect("standalone daemon should exit after takeover request");
        standalone_result
            .expect("standalone task should resolve")
            .expect("standalone daemon should shut down cleanly");

        embedded.shutdown();
        embedded.wait().await;
    }
}
