use scriptum_common::protocol::ws::{WsMessage, CURRENT_PROTOCOL_VERSION};
use serde_json::Value;
use uuid::Uuid;

const RELAY_WS_SESSION_SOURCE: &str = include_str!("../src/ws/session.rs");
const RELAY_PROTOCOL_SOURCE: &str = include_str!("../src/protocol.rs");

#[test]
fn websocket_contract_heartbeat_and_timeout_match_spec() {
    let heartbeat_interval_ms = parse_u64_const(RELAY_WS_SESSION_SOURCE, "HEARTBEAT_INTERVAL_MS");
    let heartbeat_timeout_ms = parse_u64_const(RELAY_WS_SESSION_SOURCE, "HEARTBEAT_TIMEOUT_MS");
    let max_frame_bytes = parse_u64_const(RELAY_WS_SESSION_SOURCE, "MAX_FRAME_BYTES");

    assert_eq!(heartbeat_interval_ms, 15_000);
    assert_eq!(heartbeat_timeout_ms, 10_000);
    assert_eq!(max_frame_bytes, 262_144);
    assert!(
        heartbeat_timeout_ms < heartbeat_interval_ms,
        "pong timeout must be shorter than heartbeat interval",
    );
}

#[test]
fn websocket_contract_protocol_version_is_scriptum_sync_v1() {
    assert!(
        RELAY_PROTOCOL_SOURCE.contains("pub const CURRENT_VERSION: &str = \"scriptum-sync.v1\"")
    );
    assert!(RELAY_PROTOCOL_SOURCE.contains("const SUPPORTED_VERSIONS"));
    assert!(RELAY_PROTOCOL_SOURCE.contains("CURRENT_VERSION"));
    assert!(RELAY_PROTOCOL_SOURCE.contains("\"scriptum-sync.v0\""));
}

#[test]
fn websocket_contract_message_shapes_match_spec() {
    let doc_id = Uuid::new_v4();
    let client_id = Uuid::new_v4();
    let client_update_id = Uuid::new_v4();

    let samples = [
        (
            WsMessage::Hello {
                protocol_version: CURRENT_PROTOCOL_VERSION.to_string(),
                session_token: "session-token".to_string(),
                resume_token: Some("resume-token".to_string()),
            },
            "hello",
            &["type", "protocol_version", "session_token", "resume_token"][..],
        ),
        (
            WsMessage::HelloAck {
                server_time: "2026-02-07T00:00:00Z".to_string(),
                resume_accepted: true,
                resume_token: "resume-next".to_string(),
                resume_expires_at: "2026-02-07T00:10:00Z".to_string(),
            },
            "hello_ack",
            &["type", "server_time", "resume_accepted", "resume_token", "resume_expires_at"][..],
        ),
        (
            WsMessage::Subscribe { doc_id, last_server_seq: Some(7) },
            "subscribe",
            &["type", "doc_id", "last_server_seq"][..],
        ),
        (
            WsMessage::YjsUpdate {
                doc_id,
                client_id,
                client_update_id,
                base_server_seq: 7,
                payload_b64: "AQID".to_string(),
            },
            "yjs_update",
            &["type", "doc_id", "client_id", "client_update_id", "base_server_seq", "payload_b64"]
                [..],
        ),
        (
            WsMessage::Ack { doc_id, client_update_id, server_seq: 8, applied: true },
            "ack",
            &["type", "doc_id", "client_update_id", "server_seq", "applied"][..],
        ),
        (
            WsMessage::Snapshot { doc_id, snapshot_seq: 9, payload_b64: "CQgH".to_string() },
            "snapshot",
            &["type", "doc_id", "snapshot_seq", "payload_b64"][..],
        ),
        (
            WsMessage::AwarenessUpdate {
                doc_id,
                peers: vec![serde_json::json!({ "client_id": client_id })],
            },
            "awareness_update",
            &["type", "doc_id", "peers"][..],
        ),
        (
            WsMessage::Error {
                code: "AUTH_INVALID_TOKEN".to_string(),
                message: "invalid token".to_string(),
                retryable: false,
                doc_id: Some(doc_id),
            },
            "error",
            &["type", "code", "message", "retryable", "doc_id"][..],
        ),
    ];

    for (message, expected_type, expected_keys) in samples {
        let value = serde_json::to_value(message).expect("ws message should serialize");
        assert_eq!(value["type"], expected_type);
        for key in expected_keys {
            assert!(
                value.get(key).is_some(),
                "serialized `{expected_type}` frame must include `{key}`",
            );
        }
    }
}

#[test]
fn websocket_contract_optional_fields_are_omitted_when_absent() {
    let hello_without_resume = WsMessage::Hello {
        protocol_version: CURRENT_PROTOCOL_VERSION.to_string(),
        session_token: "session-token".to_string(),
        resume_token: None,
    };
    let error_without_doc = WsMessage::Error {
        code: "AUTH_INVALID_TOKEN".to_string(),
        message: "invalid token".to_string(),
        retryable: false,
        doc_id: None,
    };

    let hello_json = serde_json::to_value(hello_without_resume).expect("hello should serialize");
    let error_json = serde_json::to_value(error_without_doc).expect("error should serialize");

    assert!(object_keys(&hello_json).contains(&"protocol_version".to_string()));
    assert!(!object_keys(&hello_json).contains(&"resume_token".to_string()));
    assert!(!object_keys(&error_json).contains(&"doc_id".to_string()));
}

fn object_keys(value: &Value) -> Vec<String> {
    let mut keys =
        value.as_object().expect("value should be an object").keys().cloned().collect::<Vec<_>>();
    keys.sort();
    keys
}

fn parse_u64_const(source: &str, name: &str) -> u64 {
    let needle = format!("const {name}:");
    let index = source.find(&needle).expect("constant must be declared");
    let line = source[index..].lines().next().expect("constant declaration line must exist");
    let raw_value = line
        .split('=')
        .nth(1)
        .expect("constant must have assignment")
        .trim()
        .trim_end_matches(';')
        .replace('_', "");
    raw_value
        .parse::<u64>()
        .unwrap_or_else(|error| panic!("failed to parse `{name}` from `{line}`: {error}"))
}
