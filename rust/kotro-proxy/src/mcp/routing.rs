//! Streamable HTTP routing headers (SEP-2243 / MCP 2026-07-28).
//!
//! Clients MUST send `MCP-Protocol-Version`, `Mcp-Method`, and (when the
//! operation names a primitive) `Mcp-Name`. Servers MUST reject requests where
//! those headers disagree with the JSON-RPC body. Kotro's mcp-wrap currently
//! acts as an HTTP *client* toward remote MCP servers; [`validate_agreement`]
//! is the shared check used for emission tests today and for gateway
//! termination when that path lands.

use serde_json::Value;

/// Protocol version Kotro speaks on Streamable HTTP.
pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";

/// Routing headers derived from a JSON-RPC request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingHeaders {
    pub protocol_version: &'static str,
    pub method: String,
    pub name: Option<String>,
}

impl RoutingHeaders {
    /// Build the headers Kotro MUST emit for an outbound Streamable HTTP POST.
    pub fn from_rpc(method: &str, params: &Value) -> Self {
        Self {
            protocol_version: MCP_PROTOCOL_VERSION,
            method: method.to_string(),
            name: routing_name(method, params),
        }
    }
}

/// Primitive name for `Mcp-Name` when the method addresses a named resource.
pub fn routing_name(method: &str, params: &Value) -> Option<String> {
    match method {
        "tools/call" | "prompts/get" => params
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "resources/read" | "resources/subscribe" | "resources/unsubscribe" => params
            .get("uri")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

/// Reject header↔body disagreement (server-side SEP-2243 rule).
///
/// - `Mcp-Method` MUST equal the JSON-RPC `method`.
/// - When a primitive name is required, `Mcp-Name` MUST equal the body name/uri.
/// - When no primitive name applies, `Mcp-Name` MUST be absent or empty.
pub fn validate_agreement(
    method_header: Option<&str>,
    name_header: Option<&str>,
    method: &str,
    params: &Value,
) -> Result<(), String> {
    let expected = RoutingHeaders::from_rpc(method, params);
    let hdr_method = method_header.map(str::trim).filter(|s| !s.is_empty());
    match hdr_method {
        None => return Err("missing Mcp-Method header".into()),
        Some(m) if m != expected.method => {
            return Err(format!(
                "Mcp-Method '{m}' disagrees with body method '{}'",
                expected.method
            ));
        }
        Some(_) => {}
    }
    let hdr_name = name_header.map(str::trim).filter(|s| !s.is_empty());
    match (&expected.name, hdr_name) {
        (Some(want), Some(got)) if want == got => Ok(()),
        (Some(want), Some(got)) => Err(format!(
            "Mcp-Name '{got}' disagrees with body primitive '{want}'"
        )),
        (Some(want), None) => Err(format!("missing Mcp-Name header (expected '{want}')")),
        (None, Some(got)) => Err(format!(
            "unexpected Mcp-Name '{got}' for method '{}'",
            expected.method
        )),
        (None, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tools_call_emits_method_and_name() {
        let h = RoutingHeaders::from_rpc(
            "tools/call",
            &json!({"name": "search", "arguments": {"q": "otters"}}),
        );
        assert_eq!(h.protocol_version, MCP_PROTOCOL_VERSION);
        assert_eq!(h.method, "tools/call");
        assert_eq!(h.name.as_deref(), Some("search"));
    }

    #[test]
    fn resources_read_uses_uri_as_name() {
        let h = RoutingHeaders::from_rpc(
            "resources/read",
            &json!({"uri": "file:///tmp/notes.txt"}),
        );
        assert_eq!(h.name.as_deref(), Some("file:///tmp/notes.txt"));
    }

    #[test]
    fn tools_list_has_no_name() {
        let h = RoutingHeaders::from_rpc("tools/list", &json!({}));
        assert!(h.name.is_none());
    }

    #[test]
    fn agreement_accepts_matching_headers() {
        let params = json!({"name": "search", "arguments": {}});
        assert!(validate_agreement(
            Some("tools/call"),
            Some("search"),
            "tools/call",
            &params
        )
        .is_ok());
    }

    #[test]
    fn agreement_rejects_method_mismatch() {
        let err = validate_agreement(
            Some("tools/list"),
            None,
            "tools/call",
            &json!({"name": "search"}),
        )
        .unwrap_err();
        assert!(err.contains("disagrees"), "{err}");
    }

    #[test]
    fn agreement_rejects_name_mismatch() {
        let err = validate_agreement(
            Some("tools/call"),
            Some("other"),
            "tools/call",
            &json!({"name": "search"}),
        )
        .unwrap_err();
        assert!(err.contains("Mcp-Name"), "{err}");
    }

    #[test]
    fn agreement_rejects_missing_name_when_required() {
        let err =
            validate_agreement(Some("tools/call"), None, "tools/call", &json!({"name": "search"}))
                .unwrap_err();
        assert!(err.contains("missing Mcp-Name"), "{err}");
    }

    #[test]
    fn agreement_rejects_spurious_name() {
        let err =
            validate_agreement(Some("tools/list"), Some("search"), "tools/list", &json!({}))
                .unwrap_err();
        assert!(err.contains("unexpected Mcp-Name"), "{err}");
    }
}
