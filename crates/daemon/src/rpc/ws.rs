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
                let response = handle_raw_request(payload.as_bytes(), &state).await;
                if let Ok(encoded) = serde_json::to_string(&response) {
                    if socket.send(WsMessage::Text(encoded.into())).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
            WsMessage::Binary(payload) => {
                let response = handle_raw_request(payload.as_ref(), &state).await;
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
