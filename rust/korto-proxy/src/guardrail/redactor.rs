//! Request-body PII redaction — mirrors `internal/guardrail/redactor.go` (subset).

use std::sync::Arc;

use regex::Regex;
use serde_json::Value;

use crate::models::openai::content_text;
use crate::models::{anthropic::MessagesRequest, openai::ChatCompletionRequest};

use super::redaction_map::RedactionMap;

fn patterns() -> &'static [Regex] {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"AKIA[0-9A-Z]{16}",
            r#"(?i)(?:api[_-]?key|secret[_-]?key|token)\s*[:=]\s*['"]?[^\s'"]{8,}['"]?"#,
            r"sk-[a-zA-Z0-9]{20,}",
            r"sk-ant-[a-zA-Z0-9\-]{20,}",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("valid redaction regex"))
        .collect()
    })
}

fn redact_text(text: &str, map: &RedactionMap) -> String {
    let mut result = text.to_string();
    for pattern in patterns() {
        let mut rebuilt = String::new();
        let mut last = 0;
        for m in pattern.find_iter(&result) {
            rebuilt.push_str(&result[last..m.start()]);
            rebuilt.push_str(&map.placeholder_for(m.as_str()));
            last = m.end();
        }
        rebuilt.push_str(&result[last..]);
        result = rebuilt;
    }
    result
}

fn with_text(content: &Value, text: &str) -> Value {
    match content {
        Value::String(_) | Value::Null => Value::String(text.to_string()),
        Value::Array(parts) => {
            let mut out = parts.clone();
            let mut replaced = false;
            for part in &mut out {
                if part.get("type").and_then(Value::as_str) == Some("text") {
                    part["text"] = Value::String(text.to_string());
                    replaced = true;
                }
            }
            if !replaced {
                out.insert(
                    0,
                    serde_json::json!({"type": "text", "text": text}),
                );
            }
            Value::Array(out)
        }
        other => Value::String(format!("{other} {text}")),
    }
}

pub fn redact_chat_request(req: ChatCompletionRequest) -> (ChatCompletionRequest, Arc<RedactionMap>) {
    let map = Arc::new(RedactionMap::new());
    let mut out = req;
    for msg in &mut out.messages {
        let text = content_text(&msg.content);
        if text.is_empty() {
            continue;
        }
        let redacted = redact_text(&text, &map);
        msg.content = with_text(&msg.content, &redacted);
    }
    (out, map)
}

pub fn redact_messages_request(req: MessagesRequest) -> (MessagesRequest, Arc<RedactionMap>) {
    let map = Arc::new(RedactionMap::new());
    let mut out = req;
    if !out.system.is_null() {
        let text = content_text(&out.system);
        if !text.is_empty() {
            out.system = with_text(&out.system, &redact_text(&text, &map));
        }
    }
    for msg in &mut out.messages {
        let text = content_text(&msg.content);
        if text.is_empty() {
            continue;
        }
        let redacted = redact_text(&text, &map);
        msg.content = with_text(&msg.content, &redacted);
    }
    (out, map)
}
