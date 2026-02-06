// WebSocket message types for the scriptum-sync.v1 protocol.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// All message types in the scriptum-sync.v1 WebSocket protocol.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Client -> Server: initial handshake.
    Hello {
        session_token: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        resume_token: Option<String>,
    },

    /// Server -> Client: handshake acknowledgement.
    HelloAck {
        server_time: String,
        resume_accepted: bool,
    },

    /// Client -> Server: subscribe to document updates.
    Subscribe {
        doc_id: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_server_seq: Option<i64>,
    },

    /// Bidirectional: a Yjs binary update.
    YjsUpdate {
        doc_id: Uuid,
        client_id: Uuid,
        client_update_id: Uuid,
        base_server_seq: i64,
        payload_b64: String,
    },

    /// Server -> Client: acknowledgement of a client update.
    Ack {
        doc_id: Uuid,
        client_update_id: Uuid,
        server_seq: i64,
        applied: bool,
    },

    /// Server -> Client: full document snapshot.
    Snapshot {
        doc_id: Uuid,
        snapshot_seq: i64,
        payload_b64: String,
    },

    /// Bidirectional: awareness/presence updates.
    AwarenessUpdate {
        doc_id: Uuid,
        peers: Vec<serde_json::Value>,
    },

    /// Server -> Client: error.
    Error {
        code: String,
        message: String,
        retryable: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        doc_id: Option<Uuid>,
    },
}
