//! W3C Trace Context extraction from MCP `params._meta` (SEP-414).
//!
//! Spec keys (unprefixed exception to the DNS-prefix convention):
//! `traceparent`, `tracestate`, `baggage`.

use serde_json::Value;

/// Parsed W3C Trace Context carried on an MCP request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub tracestate: String,
    pub baggage: String,
}

impl TraceContext {
    /// Extract from a JSON-RPC request's `params._meta` object.
    pub fn from_rpc_params(params: &Value) -> Self {
        let meta = params.get("_meta").unwrap_or(&Value::Null);
        Self::from_meta(meta)
    }

    /// Extract from an MCP `_meta` object.
    pub fn from_meta(meta: &Value) -> Self {
        let traceparent = meta.get("traceparent").and_then(Value::as_str).unwrap_or("");
        let (trace_id, span_id) = parse_traceparent(traceparent);
        Self {
            trace_id,
            span_id,
            tracestate: meta
                .get("tracestate")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            baggage: meta
                .get("baggage")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.trace_id.is_empty() && self.span_id.is_empty()
    }
}

/// Parse a W3C `traceparent` header value.
///
/// Format: `{version}-{trace-id}-{parent-id}-{flags}` where trace-id is 32 hex
/// chars and parent-id (span id) is 16 hex chars.
pub fn parse_traceparent(traceparent: &str) -> (String, String) {
    let parts: Vec<&str> = traceparent.trim().split('-').collect();
    if parts.len() < 4 {
        return (String::new(), String::new());
    }
    let version = parts[0];
    let trace_id = parts[1];
    let span_id = parts[2];
    if version.len() != 2
        || trace_id.len() != 32
        || span_id.len() != 16
        || !trace_id.chars().all(|c| c.is_ascii_hexdigit())
        || !span_id.chars().all(|c| c.is_ascii_hexdigit())
        || trace_id.chars().all(|c| c == '0')
        || span_id.chars().all(|c| c == '0')
    {
        return (String::new(), String::new());
    }
    (trace_id.to_ascii_lowercase(), span_id.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_valid_traceparent() {
        let (tid, sid) = parse_traceparent(
            "00-0af7651916cd43dd8448eb211c80319c-00f067aa0ba902b7-01",
        );
        assert_eq!(tid, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(sid, "00f067aa0ba902b7");
    }

    #[test]
    fn rejects_all_zero_ids() {
        let (tid, sid) =
            parse_traceparent("00-00000000000000000000000000000000-0000000000000000-01");
        assert!(tid.is_empty());
        assert!(sid.is_empty());
    }

    #[test]
    fn from_rpc_params_reads_meta() {
        let params = json!({
            "name": "get_weather",
            "arguments": {"location": "NYC"},
            "_meta": {
                "traceparent": "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                "tracestate": "rojo=00f067aa0ba902b7",
                "baggage": "userId=alice"
            }
        });
        let ctx = TraceContext::from_rpc_params(&params);
        assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ctx.span_id, "00f067aa0ba902b7");
        assert_eq!(ctx.tracestate, "rojo=00f067aa0ba902b7");
        assert_eq!(ctx.baggage, "userId=alice");
    }
}
