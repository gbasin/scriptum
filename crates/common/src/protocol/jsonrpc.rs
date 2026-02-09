// JSON-RPC 2.0 request/response types for the daemon socket protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CURRENT_PROTOCOL_VERSION: &str = "scriptum-rpc.v1";
pub const PREVIOUS_PROTOCOL_VERSION: &str = "scriptum-rpc.v0";
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[CURRENT_PROTOCOL_VERSION, PREVIOUS_PROTOCOL_VERSION];

#[must_use]
pub fn is_supported_protocol_version(version: &str) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
}

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    pub id: RequestId,
}

/// A JSON-RPC 2.0 response (success).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: RequestId,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Request ID: integer, string, or null.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
    Null,
}

// Standard JSON-RPC error codes.
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

impl Request {
    pub fn new(method: impl Into<String>, params: Option<Value>, id: RequestId) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            protocol_version: Some(CURRENT_PROTOCOL_VERSION.to_string()),
            method: method.into(),
            params,
            id,
        }
    }
}

impl Response {
    pub fn success(id: RequestId, result: Value) -> Self {
        Self { jsonrpc: "2.0".to_string(), result: Some(result), error: None, id }
    }

    pub fn error(id: RequestId, error: RpcError) -> Self {
        Self { jsonrpc: "2.0".to_string(), result: None, error: Some(error), id }
    }
}
