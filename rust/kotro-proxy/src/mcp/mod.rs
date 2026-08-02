//! MCP action plane — governed proxying of Model Context Protocol traffic.
//!
//! `kotro-proxy mcp-wrap --name <server> -- <command> [args…]` wraps a stdio
//! MCP server; `--url` wraps a remote Streamable HTTP server. Either way the
//! client speaks stdio to Kotro, and Kotro enforces:
//!
//! - tool metadata pinning + rug-pull quarantine on `tools/list`,
//! - JSON Schema validation of `tools/call` arguments,
//! - deny / ask / allow policy before execution (deny wins),
//! - the multi-plane kill switch (`tools` / `all` scopes),
//! - flight-recorder reporting through the proxy control API.

pub mod list_cache;
pub mod pin;
pub mod protect;
pub mod report;
pub mod schema;
pub mod trace;
pub mod wrap;

use serde_json::Value;

/// JSON-RPC error codes used by the governance layer.
pub const ERR_POLICY_DENIED: i64 = -32001;
/// Operator kill switch. Uses `-32050` (implementation-defined server-error
/// range) rather than `-32002`, which older MCP clients treat as the legacy
/// "resource not found" semantic before that meaning moved to `-32602`.
pub const ERR_KILL_SWITCH: i64 = -32050;
pub const ERR_INVALID_ARGS: i64 = -32602;

/// Build a JSON-RPC error response for a blocked request.
pub fn rpc_error(id: &Value, code: i64, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}

/// A parsed inbound JSON-RPC message (request or notification).
pub struct RpcMessage {
    pub raw: Value,
    pub id: Option<Value>,
    pub method: Option<String>,
}

pub fn parse_message(line: &str) -> Option<RpcMessage> {
    // Reject duplicate JSON object keys before serde collapses them.
    let raw = kotro_schema::parse_json_rejecting_duplicates(line.as_bytes()).ok()?;
    let id = raw.get("id").cloned();
    let method = raw.get("method").and_then(Value::as_str).map(str::to_string);
    Some(RpcMessage { raw, id, method })
}

/// Stable string key for a JSON-RPC id (number or string).
pub fn id_key(id: &Value) -> String {
    match id {
        Value::String(s) => format!("s:{s}"),
        Value::Number(n) => format!("n:{n}"),
        other => format!("v:{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_and_notification() {
        let req = parse_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#)
            .unwrap();
        assert_eq!(req.method.as_deref(), Some("tools/call"));
        assert!(req.id.is_some());

        let note = parse_message(r#"{"jsonrpc":"2.0","method":"notifications/progress"}"#).unwrap();
        assert!(note.id.is_none());
    }

    #[test]
    fn error_response_shape() {
        let raw = rpc_error(&serde_json::json!(7), ERR_POLICY_DENIED, "denied");
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["error"]["code"], ERR_POLICY_DENIED);
    }
}
