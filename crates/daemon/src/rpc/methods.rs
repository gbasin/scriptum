use scriptum_common::protocol::jsonrpc::{
    Request, RequestId, Response, RpcError, INTERNAL_ERROR, INVALID_REQUEST, METHOD_NOT_FOUND,
    PARSE_ERROR,
};
use serde_json::json;

#[derive(Clone, Debug, Default)]
pub struct RpcServerState;

pub async fn handle_raw_request(raw: &[u8], state: &RpcServerState) -> Response {
    let request = match serde_json::from_slice::<Request>(raw) {
        Ok(request) => request,
        Err(error) => {
            return Response::error(
                RequestId::Null,
                RpcError {
                    code: PARSE_ERROR,
                    message: "Parse error".to_string(),
                    data: Some(json!({ "reason": error.to_string() })),
                },
            );
        }
    };

    if request.jsonrpc != "2.0" {
        return Response::error(
            request.id,
            RpcError { code: INVALID_REQUEST, message: "Invalid Request".to_string(), data: None },
        );
    }

    dispatch_request(request, state).await
}

pub async fn dispatch_request(request: Request, _state: &RpcServerState) -> Response {
    match request.method.as_str() {
        "rpc.ping" => Response::success(
            request.id,
            json!({
                "ok": true,
            }),
        ),
        "rpc.internal_error" => Response::error(
            request.id,
            RpcError { code: INTERNAL_ERROR, message: "Internal error".to_string(), data: None },
        ),
        _ => Response::error(
            request.id,
            RpcError {
                code: METHOD_NOT_FOUND,
                message: "Method not found".to_string(),
                data: None,
            },
        ),
    }
}
