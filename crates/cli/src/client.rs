use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(unix)]
use tokio::time::timeout;

pub const DAEMON_NOT_RUNNING_EXIT_CODE: i32 = 10;

const SOCKET_RELATIVE_PATH: &str = ".scriptum/daemon.sock";
const DEFAULT_TIMEOUT_SECS: u64 = 3;

#[derive(Debug)]
pub struct DaemonUnavailable {
    socket_path: PathBuf,
    source: io::Error,
}

impl DaemonUnavailable {
    fn new(socket_path: PathBuf, source: io::Error) -> Self {
        Self { socket_path, source }
    }

    pub fn exit_code(&self) -> i32 {
        DAEMON_NOT_RUNNING_EXIT_CODE
    }
}

impl fmt::Display for DaemonUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "daemon is not running (socket `{}`); use exit code {}",
            self.socket_path.display(),
            self.exit_code()
        )
    }
}

impl std::error::Error for DaemonUnavailable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a, P> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: P,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<R> {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Value,
    result: Option<R>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

#[derive(Debug)]
pub struct DaemonClient {
    socket_path: PathBuf,
    timeout: Duration,
    next_request_id: AtomicU64,
}

impl Default for DaemonClient {
    fn default() -> Self {
        Self::new(default_socket_path())
    }
}

impl DaemonClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            next_request_id: AtomicU64::new(1),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn call<P, R>(&self, method: &str, params: P) -> Result<R>
    where
        P: Serialize + Clone,
        R: DeserializeOwned,
    {
        let id = self.next_request_id.fetch_add(1, Ordering::SeqCst);

        match self.call_once(id, method, params.clone()).await {
            Ok(response) => Ok(response),
            Err(first_error) => {
                // Retry once for transient socket drops / daemon restarts.
                self.call_once(id, method, params).await.map_err(|second_error| {
                    second_error.context(format!(
                        "json-rpc call failed after retry; first error: {first_error:#}"
                    ))
                })
            }
        }
    }

    async fn call_once<P, R>(&self, id: u64, method: &str, params: P) -> Result<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        #[cfg(unix)]
        {
            let request = JsonRpcRequest { jsonrpc: "2.0", id, method, params };
            let mut payload =
                serde_json::to_vec(&request).context("failed to serialize json-rpc request")?;
            payload.push(b'\n');

            let stream = timeout(self.timeout, UnixStream::connect(&self.socket_path))
                .await
                .context("timed out connecting to daemon socket")?
                .map_err(|err| {
                    if is_daemon_unavailable_kind(err.kind()) {
                        anyhow!(DaemonUnavailable::new(self.socket_path.clone(), err))
                    } else {
                        anyhow!(err)
                    }
                })
                .with_context(|| {
                    format!("failed to connect to daemon socket `{}`", self.socket_path.display())
                })?;

            let (read_half, mut write_half) = stream.into_split();
            timeout(self.timeout, write_half.write_all(&payload))
                .await
                .context("timed out writing json-rpc request")?
                .context("failed writing json-rpc request to daemon socket")?;
            timeout(self.timeout, write_half.flush())
                .await
                .context("timed out flushing json-rpc request")?
                .context("failed flushing json-rpc request to daemon socket")?;

            let mut reader = BufReader::new(read_half);
            let mut response_line = Vec::new();
            timeout(self.timeout, reader.read_until(b'\n', &mut response_line))
                .await
                .context("timed out waiting for json-rpc response")?
                .context("failed reading json-rpc response from daemon socket")?;

            if response_line.is_empty() {
                anyhow::bail!("daemon returned an empty json-rpc response");
            }

            let response: JsonRpcResponse<R> = serde_json::from_slice(&response_line)
                .context("failed to decode daemon json-rpc response")?;

            if let Some(error) = response.error {
                anyhow::bail!(
                    "daemon json-rpc error {}: {} (data: {})",
                    error.code,
                    error.message,
                    error.data.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string())
                );
            }

            return response.result.context("daemon json-rpc response missing `result` field");
        }

        #[cfg(not(unix))]
        {
            let _ = id;
            let _ = method;
            let _ = params;
            anyhow::bail!("windows named pipe transport is not implemented yet")
        }
    }
}

pub fn daemon_unavailable_exit_code(error: &anyhow::Error) -> Option<i32> {
    error.downcast_ref::<DaemonUnavailable>().map(DaemonUnavailable::exit_code)
}

fn default_socket_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(SOCKET_RELATIVE_PATH)
}

fn is_daemon_unavailable_kind(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused)
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    use super::{daemon_unavailable_exit_code, DaemonClient, DAEMON_NOT_RUNNING_EXIT_CODE};

    #[tokio::test]
    async fn calls_json_rpc_over_unix_socket() {
        let socket_path = unique_socket_path("json-rpc-call");
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("skipping unix socket test: bind is not permitted in this environment");
                return;
            }
            Err(error) => panic!("listener should bind: {error}"),
        };

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept should succeed");
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut request = Vec::new();
            reader.read_until(b'\n', &mut request).await.expect("request should be readable");

            let response = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "agent_id": "claude-1" }
            })
            .to_string()
                + "\n";
            write_half.write_all(response.as_bytes()).await.expect("response write should succeed");
        });

        let client = DaemonClient::new(socket_path.clone());
        let result: serde_json::Value =
            client.call("agent.whoami", json!({})).await.expect("json-rpc call should succeed");
        assert_eq!(result["agent_id"], "claude-1");

        server.await.expect("server should finish");
        cleanup_socket_file(&socket_path);
    }

    #[tokio::test]
    async fn retries_once_when_first_connection_drops() {
        let socket_path = unique_socket_path("json-rpc-retry");
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("skipping unix socket test: bind is not permitted in this environment");
                return;
            }
            Err(error) => panic!("listener should bind: {error}"),
        };
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_server = Arc::clone(&attempts);

        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.expect("accept should succeed");
                let attempt = attempts_for_server.fetch_add(1, Ordering::SeqCst);

                if attempt == 0 {
                    // Drop first connection immediately to force a retry.
                    drop(stream);
                    continue;
                }

                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut request = Vec::new();
                reader.read_until(b'\n', &mut request).await.expect("request should be readable");

                let response = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": { "ok": true }
                })
                .to_string()
                    + "\n";
                write_half
                    .write_all(response.as_bytes())
                    .await
                    .expect("response write should succeed");
                return;
            }
        });

        let client = DaemonClient::new(socket_path.clone());
        let result: serde_json::Value = client
            .call("workspace.list", json!({}))
            .await
            .expect("json-rpc call should succeed after retry");
        assert_eq!(result["ok"], true);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);

        server.await.expect("server should finish");
        cleanup_socket_file(&socket_path);
    }

    #[tokio::test]
    async fn tags_missing_socket_as_daemon_unavailable() {
        let socket_path = unique_socket_path("missing-daemon");
        cleanup_socket_file(&socket_path);

        let client = DaemonClient::new(socket_path.clone());
        let error = client
            .call::<_, serde_json::Value>("agent.whoami", json!({}))
            .await
            .expect_err("missing socket should fail");

        assert_eq!(daemon_unavailable_exit_code(&error), Some(DAEMON_NOT_RUNNING_EXIT_CODE));
    }

    fn unique_socket_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("scriptum-{prefix}-{nanos}.sock"))
    }

    fn cleanup_socket_file(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
    }
}
