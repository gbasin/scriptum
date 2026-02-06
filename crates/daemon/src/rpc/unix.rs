use anyhow::{Context, Result};
use tokio::io::{self, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixListener;
use tracing::warn;

use crate::rpc::methods::{handle_raw_request, RpcServerState};

#[cfg(windows)]
pub const WINDOWS_NAMED_PIPE_PATH: &str = r"\\.\pipe\scriptum-daemon";

/// Serve JSON-RPC 2.0 over Unix domain sockets.
///
/// Framing is newline-delimited JSON, matching the CLI transport.
#[cfg(unix)]
pub async fn serve_unix(listener: UnixListener, state: RpcServerState) -> Result<()> {
    loop {
        let (stream, _) =
            listener.accept().await.context("failed to accept unix rpc connection")?;
        let connection_state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, connection_state).await {
                warn!(?error, "unix rpc connection failed");
            }
        });
    }
}

/// Serve JSON-RPC 2.0 over Windows named pipe `\\.\pipe\scriptum-daemon`.
#[cfg(windows)]
pub async fn serve_named_pipe(state: RpcServerState) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    loop {
        let server = ServerOptions::new()
            .create(WINDOWS_NAMED_PIPE_PATH)
            .with_context(|| format!("failed to create named pipe `{WINDOWS_NAMED_PIPE_PATH}`"))?;
        server.connect().await.context("failed to accept windows named-pipe rpc connection")?;

        let connection_state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_connection(server, connection_state).await {
                warn!(?error, "windows named-pipe rpc connection failed");
            }
        });
    }
}

/// Handle a single RPC stream. Each request line yields one response line.
pub async fn serve_connection<IO>(stream: IO, state: RpcServerState) -> Result<()>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let (read_half, mut write_half) = io::split(stream);
    let mut reader = BufReader::new(read_half);

    loop {
        let mut request_line = Vec::new();
        let bytes_read = reader
            .read_until(b'\n', &mut request_line)
            .await
            .context("failed to read json-rpc request")?;

        if bytes_read == 0 {
            return Ok(());
        }

        trim_line_endings(&mut request_line);
        if request_line.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }

        let response = handle_raw_request(&request_line, &state).await;
        let mut encoded =
            serde_json::to_vec(&response).context("failed to serialize json-rpc response")?;
        encoded.push(b'\n');

        write_half.write_all(&encoded).await.context("failed to write json-rpc response")?;
        write_half.flush().await.context("failed to flush json-rpc response")?;
    }
}

fn trim_line_endings(line: &mut Vec<u8>) {
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        io,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use scriptum_common::protocol::jsonrpc::{Request, RequestId, Response};
    use serde_json::json;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::{UnixListener, UnixStream},
    };

    use super::{serve_unix, RpcServerState};

    #[tokio::test]
    async fn accepts_concurrent_unix_connections() {
        let socket_path = unique_socket_path("rpc-concurrency");
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("skipping unix socket test: bind is not permitted in this environment");
                return;
            }
            Err(error) => panic!("failed to bind unix socket: {error}"),
        };

        let server =
            tokio::spawn(async move { serve_unix(listener, RpcServerState::default()).await });

        let mut clients = Vec::new();
        for client_id in 0_i64..8_i64 {
            let socket_path = socket_path.clone();
            clients.push(tokio::spawn(async move {
                let request = Request::new(
                    "rpc.ping",
                    Some(json!({ "client_id": client_id })),
                    RequestId::Number(client_id),
                );
                rpc_call(&socket_path, request).await
            }));
        }

        for (expected_id, task) in (0_i64..8_i64).zip(clients) {
            let response = task.await.expect("client call should complete");
            assert_eq!(response.id, RequestId::Number(expected_id));
            assert!(response.error.is_none(), "expected success response: {response:?}");
            assert_eq!(response.result, Some(json!({ "ok": true })));
        }

        server.abort();
        let _ = server.await;
        cleanup_socket_file(&socket_path);
    }

    #[tokio::test]
    async fn keeps_connection_open_for_multiple_requests() {
        let socket_path = unique_socket_path("rpc-multi-request");
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("skipping unix socket test: bind is not permitted in this environment");
                return;
            }
            Err(error) => panic!("failed to bind unix socket: {error}"),
        };

        let server =
            tokio::spawn(async move { serve_unix(listener, RpcServerState::default()).await });
        let stream = UnixStream::connect(&socket_path).await.expect("client should connect");
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let ping = Request::new("rpc.ping", Some(json!({})), RequestId::Number(1));
        write_request(&mut write_half, &ping).await;
        let ping_response = read_response(&mut reader).await;
        assert_eq!(ping_response.result, Some(json!({ "ok": true })));
        assert!(ping_response.error.is_none());

        let unknown = Request::new("rpc.unknown", Some(json!({})), RequestId::Number(2));
        write_request(&mut write_half, &unknown).await;
        let unknown_response = read_response(&mut reader).await;
        assert!(unknown_response.result.is_none());
        assert_eq!(unknown_response.error.expect("error should be present").code, -32601);

        server.abort();
        let _ = server.await;
        cleanup_socket_file(&socket_path);
    }

    async fn rpc_call(socket_path: &Path, request: Request) -> Response {
        let stream = UnixStream::connect(socket_path).await.expect("client should connect");
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        write_request(&mut write_half, &request).await;
        read_response(&mut reader).await
    }

    async fn write_request(write_half: &mut tokio::net::unix::OwnedWriteHalf, request: &Request) {
        let mut encoded =
            serde_json::to_vec(request).expect("request should serialize for test transport");
        encoded.push(b'\n');
        write_half.write_all(&encoded).await.expect("request write should succeed");
        write_half.flush().await.expect("request flush should succeed");
    }

    async fn read_response(reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> Response {
        let mut response_line = Vec::new();
        reader.read_until(b'\n', &mut response_line).await.expect("response should be readable");
        serde_json::from_slice::<Response>(&response_line).expect("response should decode")
    }

    fn unique_socket_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("scriptum-{prefix}-{nanos}.sock"))
    }

    fn cleanup_socket_file(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}
