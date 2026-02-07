use axum::extract::ws::{Message, WebSocket};
use scriptum_common::protocol::ws::WsMessage;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HelloMessage {
    pub session_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloAckMessage {
    pub server_time: String,
    pub resume_accepted: bool,
    pub resume_token: String,
    pub resume_expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscribeMessage {
    pub doc_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_server_seq: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YjsUpdateMessage {
    pub doc_id: Uuid,
    pub client_id: Uuid,
    pub client_update_id: Uuid,
    pub base_server_seq: i64,
    pub payload_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AckMessage {
    pub doc_id: Uuid,
    pub client_update_id: Uuid,
    pub server_seq: i64,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotMessage {
    pub doc_id: Uuid,
    pub snapshot_seq: i64,
    pub payload_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AwarenessUpdateMessage {
    pub doc_id: Uuid,
    pub peers: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorMessage {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_id: Option<Uuid>,
}

pub fn decode_message(raw: &str) -> Result<WsMessage, serde_json::Error> {
    serde_json::from_str::<WsMessage>(raw)
}

pub fn encode_message(message: &WsMessage) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}

pub async fn send_ws_message(socket: &mut WebSocket, message: &WsMessage) -> Result<(), ()> {
    let encoded = encode_message(message).map_err(|_| ())?;
    socket.send(Message::Text(encoded.into())).await.map_err(|_| ())
}
