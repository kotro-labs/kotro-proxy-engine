//! Request-body PII redaction — mirrors `internal/guardrail/redactor.go` (subset).

use std::sync::Arc;

use regex::Regex;
use serde_json::Value;

use crate::models::openai::content_text;
use crate::models::{anthropic::MessagesRequest, openai::ChatCompletionRequest};

use super::redaction_map::RedactionMap;

/// Mirrors the pattern set in `internal/guardrail/pattern.go` (Go reference).
/// Keep these two lists in sync -- see `redactor_test.go` on the Go side
/// and the tests below for the parity this depends on.
fn patterns() -> &'static [Regex] {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"AKIA[0-9A-Z]{16}",
            r#"(?i)(?:password|passwd|pwd)\s*[:=]\s*['"]?[^\s'"]{4,}['"]?"#,
            r#"(?i)(?:api[_-]?key|secret[_-]?key|token)\s*[:=]\s*['"]?[^\s'"]{8,}['"]?"#,
            r"postgres(?:ql)?://[^\s]+",
            r"mysql://[^\s]+",
            r"mongodb(?:\+srv)?://[^\s]+",
            r"redis://[^\s]+",
            r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}",
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat_request(role: &str, content: &str) -> ChatCompletionRequest {
        serde_json::from_value(json!({
            "model": "gpt-4",
            "stream": true,
            "messages": [{"role": role, "content": content}],
        }))
        .expect("valid ChatCompletionRequest fixture")
    }

    fn messages_request(system: &str, role: &str, content: &str) -> MessagesRequest {
        serde_json::from_value(json!({
            "model": "claude-3-5-sonnet-20241022",
            "stream": true,
            "system": system,
            "messages": [{"role": role, "content": content}],
        }))
        .expect("valid MessagesRequest fixture")
    }

    /// Each pattern below mirrors one row in `internal/guardrail/pattern.go`
    /// on the Go side -- this is the parity check for "redaction
    /// correctness" called out in docs/roadmap/next-steps.md P1. If Go adds
    /// a pattern, add both the pattern (above) and a case here.
    #[test]
    fn redacts_aws_access_key() {
        let map = RedactionMap::new();
        let out = redact_text("key is AKIAIOSFODNN7EXAMPLE end", &map);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(out.contains("[REDACTED_SECRET_"));
    }

    #[test]
    fn redacts_password_field() {
        let map = RedactionMap::new();
        let out = redact_text(r#"password: hunter2secret"#, &map);
        assert!(!out.contains("hunter2secret"));
    }

    #[test]
    fn redacts_generic_api_key_field() {
        let map = RedactionMap::new();
        let out = redact_text(r#"api_key = "abcdef1234567890""#, &map);
        assert!(!out.contains("abcdef1234567890"));
    }

    #[test]
    fn redacts_postgres_connection_string() {
        let map = RedactionMap::new();
        let out = redact_text("DATABASE_URL=postgres://user:pass@host:5432/db", &map);
        assert!(!out.contains("postgres://user:pass@host:5432/db"));
    }

    #[test]
    fn redacts_mysql_connection_string() {
        let map = RedactionMap::new();
        let out = redact_text("mysql://root:toor@127.0.0.1:3306/app", &map);
        assert!(!out.contains("mysql://root:toor@127.0.0.1:3306/app"));
    }

    #[test]
    fn redacts_mongodb_connection_string() {
        let map = RedactionMap::new();
        let out = redact_text("mongodb://admin:pw@cluster0.mongo.net/mydb", &map);
        assert!(!out.contains("mongodb://admin:pw@cluster0.mongo.net/mydb"));
    }

    #[test]
    fn redacts_mongodb_srv_connection_string() {
        let map = RedactionMap::new();
        let out = redact_text("mongodb+srv://admin:pw@cluster0.mongo.net/mydb", &map);
        assert!(!out.contains("mongodb+srv://admin:pw@cluster0.mongo.net/mydb"));
    }

    #[test]
    fn redacts_redis_connection_string() {
        let map = RedactionMap::new();
        let out = redact_text("redis://:pw@localhost:6379/0", &map);
        assert!(!out.contains("redis://:pw@localhost:6379/0"));
    }

    #[test]
    fn redacts_email_address() {
        let map = RedactionMap::new();
        let out = redact_text("contact jane.doe@example.com for access", &map);
        assert!(!out.contains("jane.doe@example.com"));
    }

    #[test]
    fn redacts_openai_style_sk_token() {
        let map = RedactionMap::new();
        let out = redact_text("sk-abcdefghijklmnopqrstuvwxyz123456", &map);
        assert!(!out.contains("sk-abcdefghijklmnopqrstuvwxyz123456"));
    }

    #[test]
    fn redacts_anthropic_style_sk_ant_token() {
        let map = RedactionMap::new();
        let out = redact_text("sk-ant-abcdefghijklmnopqrstuvwxyz-01", &map);
        assert!(!out.contains("sk-ant-abcdefghijklmnopqrstuvwxyz-01"));
    }

    #[test]
    fn leaves_ordinary_text_unmodified() {
        let map = RedactionMap::new();
        let text = "please review this pull request and merge if it looks good";
        assert_eq!(redact_text(text, &map), text);
        assert!(map.is_empty());
    }

    #[test]
    fn distinct_secrets_get_distinct_placeholders() {
        let map = RedactionMap::new();
        let out = redact_text(
            "aws key AKIAIOSFODNN7EXAMPLE and token sk-abcdefghijklmnopqrstuvwxyz123456",
            &map,
        );
        assert_eq!(map.len(), 2);
        assert!(out.contains("[REDACTED_SECRET_1]"));
        assert!(out.contains("[REDACTED_SECRET_2]"));
    }

    #[test]
    fn same_secret_repeated_reuses_one_placeholder() {
        let map = RedactionMap::new();
        let out = redact_text(
            "key AKIAIOSFODNN7EXAMPLE ... and again AKIAIOSFODNN7EXAMPLE",
            &map,
        );
        assert_eq!(map.len(), 1, "one distinct secret should register once, not twice");
        assert_eq!(out.matches("[REDACTED_SECRET_1]").count(), 2);
    }

    #[test]
    fn redact_chat_request_round_trips_through_redaction_map() {
        let req = chat_request("user", "my key is AKIAIOSFODNN7EXAMPLE, please don't leak it");
        let (redacted, map) = redact_chat_request(req);

        let redacted_text = content_text(&redacted.messages[0].content);
        assert!(!redacted_text.contains("AKIAIOSFODNN7EXAMPLE"));

        // Placeholder must be reversible -- this is what lets the client see
        // their own secret back in the streamed response even though the
        // upstream provider never saw it.
        let restored = map.restore(&redacted_text);
        assert!(restored.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn redact_chat_request_leaves_messages_without_secrets_untouched() {
        let req = chat_request("user", "what does this function do?");
        let (redacted, map) = redact_chat_request(req);
        assert_eq!(content_text(&redacted.messages[0].content), "what does this function do?");
        assert!(map.is_empty());
    }

    #[test]
    fn redact_messages_request_covers_system_prompt_and_turns() {
        let req = messages_request(
            "You are a deploy bot with DB URL postgres://user:pass@host/db",
            "user",
            "here's my email jane.doe@example.com, use it for the invite",
        );
        let (redacted, map) = redact_messages_request(req);

        let system_text = content_text(&redacted.system);
        assert!(!system_text.contains("postgres://user:pass@host/db"));

        let turn_text = content_text(&redacted.messages[0].content);
        assert!(!turn_text.contains("jane.doe@example.com"));

        // Both the system-prompt secret and the turn secret share one map,
        // so both are restorable on the way back to the client.
        assert_eq!(map.len(), 2);
    }
}
