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
        resume_token: String,
        resume_expires_at: String,
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
    Ack { doc_id: Uuid, client_update_id: Uuid, server_seq: i64, applied: bool },

    /// Server -> Client: full document snapshot.
    Snapshot { doc_id: Uuid, snapshot_seq: i64, payload_b64: String },

    /// Bidirectional: awareness/presence updates.
    AwarenessUpdate { doc_id: Uuid, peers: Vec<serde_json::Value> },

    /// Server -> Client: error.
    Error {
        code: String,
        message: String,
        retryable: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        doc_id: Option<Uuid>,
    },
}

#[cfg(test)]
mod tests {
    use super::WsMessage;
    use serde_json::Value;
    use uuid::Uuid;

    fn object_keys(value: &Value) -> Vec<String> {
        let mut keys = value
            .as_object()
            .expect("value should be a JSON object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    #[test]
    fn hello_message_matches_contract_shape() {
        let message = WsMessage::Hello {
            session_token: "session-123".to_string(),
            resume_token: Some("resume-456".to_string()),
        };

        let value = serde_json::to_value(message).expect("hello message should serialize");
        assert_eq!(
            object_keys(&value),
            vec!["resume_token".to_string(), "session_token".to_string(), "type".to_string()]
        );
        assert_eq!(value["type"], "hello");
    }

    #[test]
    fn hello_ack_message_matches_contract_shape() {
        let message = WsMessage::HelloAck {
            server_time: "2026-02-07T00:00:00Z".to_string(),
            resume_accepted: true,
            resume_token: "resume-next-789".to_string(),
            resume_expires_at: "2026-02-07T00:10:00Z".to_string(),
        };

        let value = serde_json::to_value(message).expect("hello_ack message should serialize");
        assert_eq!(
            object_keys(&value),
            vec![
                "resume_accepted".to_string(),
                "resume_expires_at".to_string(),
                "resume_token".to_string(),
                "server_time".to_string(),
                "type".to_string()
            ]
        );
        assert_eq!(value["type"], "hello_ack");
    }

    #[test]
    fn yjs_update_message_matches_contract_shape() {
        let message = WsMessage::YjsUpdate {
            doc_id: Uuid::nil(),
            client_id: Uuid::nil(),
            client_update_id: Uuid::nil(),
            base_server_seq: 42,
            payload_b64: "AQID".to_string(),
        };

        let value = serde_json::to_value(message).expect("yjs_update message should serialize");
        assert_eq!(
            object_keys(&value),
            vec![
                "base_server_seq".to_string(),
                "client_id".to_string(),
                "client_update_id".to_string(),
                "doc_id".to_string(),
                "payload_b64".to_string(),
                "type".to_string()
            ]
        );
        assert_eq!(value["type"], "yjs_update");
    }

    #[test]
    fn ack_message_matches_contract_shape() {
        let message = WsMessage::Ack {
            doc_id: Uuid::nil(),
            client_update_id: Uuid::nil(),
            server_seq: 7,
            applied: true,
        };

        let value = serde_json::to_value(message).expect("ack message should serialize");
        assert_eq!(
            object_keys(&value),
            vec![
                "applied".to_string(),
                "client_update_id".to_string(),
                "doc_id".to_string(),
                "server_seq".to_string(),
                "type".to_string()
            ]
        );
        assert_eq!(value["type"], "ack");
    }

    #[test]
    fn awareness_update_message_matches_contract_shape() {
        let message = WsMessage::AwarenessUpdate {
            doc_id: Uuid::nil(),
            peers: vec![serde_json::json!({
                "client_id": "peer-1",
                "name": "Alice"
            })],
        };

        let value =
            serde_json::to_value(message).expect("awareness_update message should serialize");
        assert_eq!(
            object_keys(&value),
            vec!["doc_id".to_string(), "peers".to_string(), "type".to_string()]
        );
        assert_eq!(value["type"], "awareness_update");
    }

    #[test]
    fn error_message_omits_doc_id_when_absent() {
        let message = WsMessage::Error {
            code: "AUTH_INVALID_TOKEN".to_string(),
            message: "token invalid".to_string(),
            retryable: false,
            doc_id: None,
        };

        let value = serde_json::to_value(message).expect("error message should serialize");
        assert_eq!(
            object_keys(&value),
            vec![
                "code".to_string(),
                "message".to_string(),
                "retryable".to_string(),
                "type".to_string()
            ]
        );
        assert_eq!(value["type"], "error");
        assert!(value.get("doc_id").is_none());
    }
}
