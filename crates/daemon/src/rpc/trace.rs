use std::future::Future;

use serde_json::Value;
use uuid::Uuid;

tokio::task_local! {
    static TRACE_ID: String;
}

pub async fn with_trace_id_scope<F>(trace_id: String, future: F) -> F::Output
where
    F: Future,
{
    TRACE_ID.scope(trace_id, future).await
}

pub fn current_trace_id() -> Option<String> {
    TRACE_ID.try_with(Clone::clone).ok()
}

pub fn trace_id_from_raw_request(raw: &[u8]) -> String {
    serde_json::from_slice::<Value>(raw)
        .ok()
        .and_then(|value| trace_id_from_value(&value))
        .unwrap_or_else(generate_trace_id)
}

fn trace_id_from_value(value: &Value) -> Option<String> {
    extract_trace_id(value)
        .or_else(|| value.get("meta").and_then(extract_trace_id))
        .or_else(|| value.get("params").and_then(extract_trace_id))
        .filter(|trace_id| !trace_id.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn extract_trace_id(value: &Value) -> Option<&str> {
    value.as_object()?.get("trace_id")?.as_str()
}

fn generate_trace_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{current_trace_id, trace_id_from_raw_request, with_trace_id_scope};

    #[test]
    fn trace_id_can_be_extracted_from_top_level_field() {
        let raw = json!({
            "jsonrpc": "2.0",
            "method": "rpc.ping",
            "trace_id": "trace-top-level-123",
            "id": 1
        });

        assert_eq!(trace_id_from_raw_request(raw.to_string().as_bytes()), "trace-top-level-123");
    }

    #[test]
    fn trace_id_can_be_extracted_from_params() {
        let raw = json!({
            "jsonrpc": "2.0",
            "method": "rpc.ping",
            "params": { "trace_id": "trace-from-params-456" },
            "id": 1
        });

        assert_eq!(trace_id_from_raw_request(raw.to_string().as_bytes()), "trace-from-params-456");
    }

    #[test]
    fn trace_id_defaults_to_generated_uuid_when_missing() {
        let raw = json!({
            "jsonrpc": "2.0",
            "method": "rpc.ping",
            "id": 1
        });

        let generated = trace_id_from_raw_request(raw.to_string().as_bytes());
        assert!(uuid::Uuid::parse_str(&generated).is_ok());
    }

    #[tokio::test]
    async fn trace_id_scope_exposes_current_trace_id() {
        let trace_id =
            with_trace_id_scope("rpc-trace-789".to_string(), async { current_trace_id() }).await;

        assert_eq!(trace_id.as_deref(), Some("rpc-trace-789"));
    }
}
