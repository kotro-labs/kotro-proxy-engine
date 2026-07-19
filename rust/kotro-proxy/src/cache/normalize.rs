//! Pre-hash normalization for cache keys.
//!
//! Applied only when building the SHA-256 key — never mutates the upstream
//! request. Goal: absorb harmless client noise (trailing whitespace, dated
//! model aliases, shuffled `tool_calls` order) so exact-match HIT rate
//! improves without widening semantic/false-hit risk.

use serde_json::{Map, Value};

/// Canonicalize a provider model id for cache partitioning.
///
/// Examples:
/// - `gpt-4o-2024-08-06` → `gpt-4o`
/// - `gpt-4o-latest` → `gpt-4o`
/// - `claude-3-5-sonnet-20241022` → `claude-3-5-sonnet`
/// - `deepseek-v4-flash` → unchanged
pub fn canonicalize_model(model: &str) -> String {
    let mut m = model.trim().to_ascii_lowercase();
    if let Some(stripped) = strip_dated_suffix(&m) {
        m = stripped;
    }
    if let Some(stripped) = strip_compact_date_suffix(&m) {
        m = stripped;
    }
    if let Some(stripped) = m.strip_suffix("-latest") {
        m = stripped.to_string();
    }
    m
}

/// `name-YYYY-MM-DD` → `name` (OpenAI-style dated snapshots).
fn strip_dated_suffix(model: &str) -> Option<String> {
    let bytes = model.as_bytes();
    if bytes.len() < 11 {
        return None;
    }
    let suffix = &model[model.len() - 11..];
    // "-YYYY-MM-DD"
    let b = suffix.as_bytes();
    if b[0] == b'-'
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[4].is_ascii_digit()
        && b[5] == b'-'
        && b[6].is_ascii_digit()
        && b[7].is_ascii_digit()
        && b[8] == b'-'
        && b[9].is_ascii_digit()
        && b[10].is_ascii_digit()
    {
        return Some(model[..model.len() - 11].to_string());
    }
    None
}

/// `name-YYYYMMDD` → `name` (Anthropic-style dated snapshots).
/// Requires a 20xx year so we don't strip arbitrary `-12345678` tails.
fn strip_compact_date_suffix(model: &str) -> Option<String> {
    if model.len() < 9 {
        return None;
    }
    let suffix = &model[model.len() - 9..];
    let b = suffix.as_bytes();
    if b[0] == b'-'
        && b[1] == b'2'
        && b[2] == b'0'
        && b[3..].iter().all(|c| c.is_ascii_digit())
    {
        return Some(model[..model.len() - 9].to_string());
    }
    None
}

/// CRLF → LF, trim trailing whitespace/newlines. Does **not** collapse
/// internal spaces (would false-hit on code / indentation).
pub fn normalize_text(s: &str) -> String {
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    s.trim_end().to_string()
}

/// Deep-normalize JSON used in cache key material.
pub fn normalize_value(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(normalize_text(s)),
        Value::Array(items) => Value::Array(items.iter().map(normalize_value).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                out.insert(k.clone(), normalize_value(v));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Sort a `tool_calls` array deterministically by function name, then id.
pub fn normalize_tool_calls(tool_calls: &Value) -> Value {
    let Value::Array(items) = normalize_value(tool_calls) else {
        return normalize_value(tool_calls);
    };

    let mut items = items;
    items.sort_by(|a, b| tool_call_sort_key(a).cmp(&tool_call_sort_key(b)));
    Value::Array(items)
}

fn tool_call_sort_key(call: &Value) -> (String, String, String) {
    let name = call
        .pointer("/function/name")
        .or_else(|| call.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let id = call
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let fallback = call.to_string();
    (name, id, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn model_strips_date_and_latest() {
        assert_eq!(canonicalize_model("gpt-4o-2024-08-06"), "gpt-4o");
        assert_eq!(canonicalize_model("GPT-4o-latest"), "gpt-4o");
        assert_eq!(
            canonicalize_model("claude-3-5-sonnet-20241022"),
            "claude-3-5-sonnet"
        );
        assert_eq!(canonicalize_model("deepseek-v4-flash"), "deepseek-v4-flash");
    }

    #[test]
    fn text_trims_trailing_only() {
        assert_eq!(normalize_text("hello  \n\n"), "hello");
        assert_eq!(normalize_text("  indented"), "  indented");
        assert_eq!(normalize_text("a\r\nb\r\n"), "a\nb");
    }

    #[test]
    fn tool_calls_sorted_by_function_name() {
        let shuffled = json!([
            {"id": "2", "type": "function", "function": {"name": "write", "arguments": "{}"}},
            {"id": "1", "type": "function", "function": {"name": "read", "arguments": "{}"}}
        ]);
        let ordered = json!([
            {"id": "1", "type": "function", "function": {"name": "read", "arguments": "{}"}},
            {"id": "2", "type": "function", "function": {"name": "write", "arguments": "{}"}}
        ]);
        assert_eq!(normalize_tool_calls(&shuffled), normalize_tool_calls(&ordered));
    }
}
