use std::{
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use scriptum_common::protocol::jsonrpc::{Request, RequestId, Response};
use scriptum_daemon::rpc::{methods::RpcServerState, unix::serve_unix, ws};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, UnixListener, UnixStream},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

#[tokio::test]
async fn unix_socket_and_ws_rpc_share_json_rpc_handlers() {
    let rpc_state = RpcServerState::default();

    let socket_path = unique_socket_path("rpc-transport");
    let unix_listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("skipping unix socket test: bind is not permitted in this environment");
            return;
        }
        Err(error) => panic!("failed to bind unix socket: {error}"),
    };

    let unix_task = tokio::spawn({
        let rpc_state = rpc_state.clone();
        async move { serve_unix(unix_listener, rpc_state).await }
    });

    let ws_listener = TcpListener::bind("127.0.0.1:0").await.expect("ws listener should bind");
    let ws_addr = ws_listener.local_addr().expect("ws listener should expose local address");
    let ws_task = tokio::spawn({
        let rpc_state = rpc_state.clone();
        async move { ws::serve(ws_listener, rpc_state).await.expect("ws rpc server should run") }
    });

    let request = Request::new("rpc.internal_error", Some(json!({})), RequestId::Number(7));

    let unix_response = send_unix_request(&socket_path, &request).await;
    let ws_response = send_ws_request(&format!("ws://{ws_addr}/rpc"), &request).await;

    assert_eq!(unix_response.id, request.id);
    assert_eq!(ws_response.id, request.id);
    assert_eq!(unix_response.result, ws_response.result);

    let unix_error = unix_response.error.expect("unix response should contain error");
    let ws_error = ws_response.error.expect("ws response should contain error");
    assert_eq!(unix_error.code, -32603);
    assert_eq!(ws_error.code, unix_error.code);
    assert_eq!(ws_error.message, unix_error.message);

    unix_task.abort();
    let _ = unix_task.await;
    ws_task.abort();
    let _ = ws_task.await;
    cleanup_socket_file(&socket_path);
}

#[tokio::test]
async fn ws_rpc_supports_batch_requests() {
    let rpc_state = RpcServerState::default();

    let ws_listener = TcpListener::bind("127.0.0.1:0").await.expect("ws listener should bind");
    let ws_addr = ws_listener.local_addr().expect("ws listener should expose local address");
    let ws_task = tokio::spawn({
        let rpc_state = rpc_state.clone();
        async move { ws::serve(ws_listener, rpc_state).await.expect("ws rpc server should run") }
    });

    let batch_request = json!([
        {
            "jsonrpc": "2.0",
            "method": "rpc.ping",
            "params": {},
            "id": 1
        },
        {
            "jsonrpc": "2.0",
            "method": "rpc.internal_error",
            "params": {},
            "id": 2
        }
    ]);

    let response = send_ws_raw_text(&format!("ws://{ws_addr}/rpc"), &batch_request.to_string()).await;
    let items = response.as_array().expect("batch response should be an array");
    assert_eq!(items.len(), 2);

    assert_eq!(items[0]["id"], json!(1));
    assert_eq!(items[0]["result"]["ok"], json!(true));

    assert_eq!(items[1]["id"], json!(2));
    assert_eq!(items[1]["error"]["code"], json!(-32603));

    ws_task.abort();
    let _ = ws_task.await;
}

#[tokio::test]
async fn ws_rpc_returns_invalid_request_for_empty_batch() {
    let rpc_state = RpcServerState::default();

    let ws_listener = TcpListener::bind("127.0.0.1:0").await.expect("ws listener should bind");
    let ws_addr = ws_listener.local_addr().expect("ws listener should expose local address");
    let ws_task = tokio::spawn({
        let rpc_state = rpc_state.clone();
        async move { ws::serve(ws_listener, rpc_state).await.expect("ws rpc server should run") }
    });

    let response = send_ws_raw_text(&format!("ws://{ws_addr}/rpc"), "[]").await;
    assert_eq!(response["error"]["code"], json!(-32600));
    assert_eq!(response["id"], Value::Null);

    ws_task.abort();
    let _ = ws_task.await;
}

async fn send_unix_request(socket_path: &Path, request: &Request) -> Response {
    let stream =
        UnixStream::connect(socket_path).await.expect("client should connect to unix socket");
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let mut encoded = serde_json::to_vec(request).expect("request should serialize");
    encoded.push(b'\n');
    write_half.write_all(&encoded).await.expect("request write should succeed");
    write_half.flush().await.expect("request flush should succeed");

    let mut response_line = Vec::new();
    reader.read_until(b'\n', &mut response_line).await.expect("response should be readable");
    serde_json::from_slice::<Response>(&response_line).expect("response should decode")
}

async fn send_ws_request(url: &str, request: &Request) -> Response {
    let (mut socket, _) = connect_async(url).await.expect("ws client should connect");
    socket
        .send(WsMessage::Text(
            serde_json::to_string(request).expect("request should encode as text").into(),
        ))
        .await
        .expect("ws request should send");

    loop {
        let next =
            socket.next().await.expect("ws should remain open").expect("ws frame should decode");
        match next {
            WsMessage::Text(payload) => {
                let response = serde_json::from_str::<Response>(&payload)
                    .expect("text response should decode");
                let _ = socket.close(None).await;
                return response;
            }
            WsMessage::Binary(payload) => {
                let response = serde_json::from_slice::<Response>(payload.as_ref())
                    .expect("binary response should decode");
                let _ = socket.close(None).await;
                return response;
            }
            WsMessage::Ping(payload) => {
                socket.send(WsMessage::Pong(payload)).await.expect("pong should send");
            }
            WsMessage::Close(_) => panic!("ws closed before returning response"),
            WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    }
}

async fn send_ws_raw_text(url: &str, payload: &str) -> Value {
    let (mut socket, _) = connect_async(url).await.expect("ws client should connect");
    socket
        .send(WsMessage::Text(payload.to_string().into()))
        .await
        .expect("ws request should send");

    loop {
        let next =
            socket.next().await.expect("ws should remain open").expect("ws frame should decode");
        match next {
            WsMessage::Text(body) => {
                let value = serde_json::from_str::<Value>(&body).expect("text response should decode");
                let _ = socket.close(None).await;
                return value;
            }
            WsMessage::Binary(body) => {
                let value =
                    serde_json::from_slice::<Value>(body.as_ref()).expect("binary response should decode");
                let _ = socket.close(None).await;
                return value;
            }
            WsMessage::Ping(payload) => {
                socket.send(WsMessage::Pong(payload)).await.expect("pong should send");
            }
            WsMessage::Close(_) => panic!("ws closed before returning response"),
            WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    }
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
