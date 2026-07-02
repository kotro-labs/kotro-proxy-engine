//! Context block dedup — mirrors `internal/compressor/context.go`.

use std::collections::HashMap;
use std::time::Duration;

use moka::sync::Cache;
use sha2::{Digest, Sha256};

use crate::models::anthropic::MessagesRequest;
use crate::models::openai::{content_text, ChatCompletionRequest};

const DEFAULT_MAX_SCOPES: u64 = 10_000;
const DEFAULT_SCOPE_TTL: Duration = Duration::from_secs(3600);

/// Isolates compressor state to a tenant/session pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Scope {
    pub tenant_id: String,
    pub session_id: String,
}

impl Scope {
    pub fn key(&self) -> String {
        format!("{}:{}", self.tenant_id, self.session_id)
    }
}

pub struct StateTracker {
    scopes: Cache<String, HashMap<String, String>>,
}

impl Default for StateTracker {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_SCOPES, DEFAULT_SCOPE_TTL)
    }
}

impl StateTracker {
    pub fn new(max_scopes: u64, idle_ttl: Duration) -> Self {
        let max_scopes = if max_scopes == 0 {
            DEFAULT_MAX_SCOPES
        } else {
            max_scopes
        };
        let idle_ttl = if idle_ttl.is_zero() {
            DEFAULT_SCOPE_TTL
        } else {
            idle_ttl
        };

        Self {
            scopes: Cache::builder()
                .max_capacity(max_scopes)
                .time_to_idle(idle_ttl)
                .build(),
        }
    }

    pub fn compress_message(&self, scope: &Scope, content: &str) -> (String, bool) {
        let blocks = split_blocks(content);
        if blocks.is_empty() {
            return (content.to_string(), false);
        }

        let scope_key = scope.key();
        let last_blocks = self
            .scopes
            .get(&scope_key)
            .unwrap_or_default();

        let mut kept = Vec::new();
        let mut changed = false;
        let mut current = HashMap::with_capacity(blocks.len());

        for block in blocks {
            let hash = block_hash(&block);
            current.insert(hash.clone(), block.clone());
            if last_blocks.get(&hash).is_some_and(|prev| prev == &block) {
                changed = true;
                continue;
            }
            kept.push(block);
        }

        self.scopes.insert(scope_key, current);

        if !changed {
            return (content.to_string(), false);
        }
        if kept.is_empty() {
            return (String::new(), true);
        }
        (kept.join("\n\n"), true)
    }

    pub fn compress_chat_request(
        &self,
        scope: &Scope,
        mut req: ChatCompletionRequest,
    ) -> ChatCompletionRequest {
        for msg in &mut req.messages {
            if msg.role != "system" && msg.role != "user" {
                continue;
            }
            let text = content_text(&msg.content);
            let (pruned, ok) = self.compress_message(scope, &text);
            if ok {
                msg.content = serde_json::Value::String(pruned);
            }
        }
        req
    }

    pub fn compress_messages_request(
        &self,
        scope: &Scope,
        mut req: MessagesRequest,
    ) -> MessagesRequest {
        if !req.system.is_null() {
            let text = content_text(&req.system);
            let (pruned, ok) = self.compress_message(scope, &text);
            if ok {
                req.system = serde_json::Value::String(pruned);
            }
        }

        for msg in &mut req.messages {
            if msg.role != "user" {
                continue;
            }
            let text = content_text(&msg.content);
            let (pruned, ok) = self.compress_message(scope, &text);
            if ok {
                msg.content = serde_json::Value::String(pruned);
            }
        }
        req
    }
}

pub fn split_blocks(content: &str) -> Vec<String> {
    let mut blocks = content
        .split("\n\n")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if blocks.is_empty() && !content.is_empty() {
        blocks.push(content.to_string());
    }
    blocks
}

fn block_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    digest[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(tenant: &str, session: &str) -> Scope {
        Scope {
            tenant_id: tenant.into(),
            session_id: session.into(),
        }
    }

    #[test]
    fn strips_unchanged_blocks_on_second_turn() {
        let tracker = StateTracker::default();
        let s = scope("tenant-a", "session-1");
        let payload = "MCP schema v1\nline1\nline2\n\nDirectory tree:\n/src";

        let (_, changed1) = tracker.compress_message(&s, payload);
        assert!(!changed1);

        let (out2, changed2) = tracker.compress_message(&s, payload);
        assert!(changed2);
        assert!(out2.is_empty());
    }

    #[test]
    fn isolates_tenant_sessions() {
        let tracker = StateTracker::default();
        let payload = "shared block\n\ncontext";
        let tenant_a = scope("tenant-a", "session-1");
        let tenant_b = scope("tenant-b", "session-1");

        tracker.compress_message(&tenant_a, payload);
        let (_, changed_b) = tracker.compress_message(&tenant_b, payload);
        assert!(!changed_b, "tenant B must not inherit tenant A compressor state");

        let (out_a, changed_a) = tracker.compress_message(&tenant_a, payload);
        assert!(changed_a);
        assert!(out_a.is_empty());
    }

    #[test]
    fn compress_messages_request_prunes_repeated_user_turn() {
        let tracker = StateTracker::default();
        let s = scope("tenant-a", "session-1");
        let req: MessagesRequest = serde_json::from_value(serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 64,
            "stream": true,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .unwrap();

        tracker.compress_messages_request(&s, req.clone());
        let second = tracker.compress_messages_request(&s, req);
        assert_eq!(
            second.messages[0].content,
            serde_json::Value::String(String::new())
        );
    }
}
