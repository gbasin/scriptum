use anyhow::{Context, Result};
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use scriptum_common::protocol::jsonrpc::{RequestId, Response, RpcError, INVALID_REQUEST};
use serde_json::{json, Value};
use tokio::net::TcpListener;

use crate::rpc::methods::{handle_raw_request, RpcServerState};

pub fn router(state: RpcServerState) -> Router {
    Router::new().route("/rpc", get(rpc_ws_route)).with_state(state)
}

pub async fn serve(listener: TcpListener, state: RpcServerState) -> Result<()> {
    axum::serve(listener, router(state)).await.context("daemon rpc websocket server failed")
}

async fn rpc_ws_route(
    ws: WebSocketUpgrade,
    State(state): State<RpcServerState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: RpcServerState) {
    while let Some(message_result) = socket.recv().await {
        let Ok(message) = message_result else {
            break;
        };

        match message {
            WsMessage::Text(payload) => {
                let response = handle_rpc_payload(payload.as_bytes(), &state).await;
                if let Ok(encoded) = serde_json::to_string(&response) {
                    if socket.send(WsMessage::Text(encoded.into())).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
            WsMessage::Binary(payload) => {
                let response = handle_rpc_payload(payload.as_ref(), &state).await;
                if let Ok(encoded) = serde_json::to_vec(&response) {
                    if socket.send(WsMessage::Binary(encoded.into())).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
            WsMessage::Ping(payload) => {
                if socket.send(WsMessage::Pong(payload)).await.is_err() {
                    break;
                }
            }
            WsMessage::Pong(_) => {}
            WsMessage::Close(_) => break,
        }
    }
}

async fn handle_rpc_payload(raw: &[u8], state: &RpcServerState) -> Value {
    let payload = match serde_json::from_slice::<Value>(raw) {
        Ok(value) => value,
        Err(_) => {
            return serde_json::to_value(handle_raw_request(raw, state).await)
                .unwrap_or(Value::Null)
        }
    };

    match payload {
        Value::Array(items) => {
            if items.is_empty() {
                return serde_json::to_value(empty_batch_response()).unwrap_or(Value::Null);
            }

            let mut responses = Vec::with_capacity(items.len());
            for item in items {
                let encoded_item = match serde_json::to_vec(&item) {
                    Ok(encoded) => encoded,
                    Err(error) => {
                        responses.push(parse_failure_response(error.to_string()));
                        continue;
                    }
                };
                responses.push(handle_raw_request(&encoded_item, state).await);
            }

            serde_json::to_value(responses).unwrap_or(Value::Null)
        }
        _ => serde_json::to_value(handle_raw_request(raw, state).await).unwrap_or(Value::Null),
    }
}

fn empty_batch_response() -> Response {
    Response::error(
        RequestId::Null,
        RpcError {
            code: INVALID_REQUEST,
            message: "Invalid Request".to_string(),
            data: Some(json!({ "reason": "batch request must not be empty" })),
        },
    )
}

fn parse_failure_response(reason: String) -> Response {
    Response::error(
        RequestId::Null,
        RpcError {
            code: INVALID_REQUEST,
            message: "Invalid Request".to_string(),
            data: Some(json!({ "reason": reason })),
        },
    )
}
