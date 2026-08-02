//! In-process cache for MCP list / resource-read results (SEP-2549).
//!
//! Honors server-declared `ttlMs` and `cacheScope`. Absent `ttlMs` is treated
//! as immediately stale (spec default). `cacheScope: "private"` keys include
//! the wrap session so entries are never shared across authorization contexts.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cache::tool::{CacheHints, CacheScope};

#[derive(Clone)]
struct Entry {
    /// Raw JSON-RPC response body as received from the upstream server
    /// (before Kotro pin/filter mutation). Re-processed on every hit so
    /// rug-pull quarantine still runs.
    body: Value,
    expires_at: Instant,
    scope: CacheScope,
}

/// Per-wrap-process result cache for cacheable MCP methods.
#[derive(Clone, Default)]
pub struct ListResultCache {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

impl ListResultCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(session: &str, method: &str, params: &Value, scope: CacheScope) -> String {
        let mut hasher = Sha256::new();
        hasher.update(method.as_bytes());
        hasher.update(b"|");
        let params_bytes = serde_json::to_vec(params).unwrap_or_default();
        hasher.update(&params_bytes);
        let params_hash = format!("{:x}", hasher.finalize());
        match scope {
            CacheScope::Public => format!("pub:{method}:{params_hash}"),
            CacheScope::Private => format!("priv:{session}:{method}:{params_hash}"),
        }
    }

    /// Look up a fresh cached response. Returns `None` when missing or stale.
    pub fn get(&self, session: &str, method: &str, params: &Value) -> Option<Value> {
        let now = Instant::now();
        let mut guard = self.inner.lock();
        // Try private key first (session-scoped), then public.
        for scope in [CacheScope::Private, CacheScope::Public] {
            let k = Self::key(session, method, params, scope);
            if let Some(entry) = guard.get(&k) {
                if now < entry.expires_at && entry.scope == scope {
                    return Some(entry.body.clone());
                }
                if now >= entry.expires_at {
                    guard.remove(&k);
                }
            }
        }
        None
    }

    /// Store a successful cacheable result when the server declared a positive TTL.
    pub fn put(&self, session: &str, method: &str, params: &Value, response: &Value) {
        let Some(result) = response.get("result") else {
            return;
        };
        // MRTR interim results and retries must not be cached.
        if result.get("resultType").and_then(Value::as_str) == Some("input_required") {
            return;
        }
        if result.get("inputResponses").is_some() || result.get("requestState").is_some() {
            return;
        }
        let hints = CacheHints::from_result(result);
        let Some(ttl) = hints.resolve_ttl(Duration::ZERO, true) else {
            return;
        };
        if ttl.is_zero() {
            return;
        }
        let k = Self::key(session, method, params, hints.cache_scope);
        self.inner.lock().insert(
            k,
            Entry {
                body: response.clone(),
                expires_at: Instant::now() + ttl,
                scope: hints.cache_scope,
            },
        );
    }

    /// Invalidate cached entries for a method (e.g. on `list_changed` notifications).
    pub fn invalidate_method(&self, method: &str) {
        let prefix_pub = format!("pub:{method}:");
        let needle = format!(":{method}:");
        self.inner.lock().retain(|k, _| {
            !(k.starts_with(&prefix_pub) || k.contains(&needle))
        });
    }

    pub fn live_count(&self) -> usize {
        let now = Instant::now();
        self.inner
            .lock()
            .values()
            .filter(|e| now < e.expires_at)
            .count()
    }
}

/// Methods whose complete results carry SEP-2549 cache hints.
pub fn is_cacheable_method(method: &str) -> bool {
    matches!(
        method,
        "server/discover"
            | "tools/list"
            | "prompts/list"
            | "resources/list"
            | "resources/templates/list"
            | "resources/read"
    )
}

/// Map a `notifications/*/list_changed` notification to the list method it invalidates.
pub fn list_changed_target(notification: &str) -> Option<&'static str> {
    match notification {
        "notifications/tools/list_changed" => Some("tools/list"),
        "notifications/prompts/list_changed" => Some("prompts/list"),
        "notifications/resources/list_changed" => Some("resources/list"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn honors_ttl_and_expires() {
        let c = ListResultCache::new();
        let params = json!({});
        let resp = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "tools": [], "ttlMs": 50, "cacheScope": "private" }
        });
        c.put("sess-a", "tools/list", &params, &resp);
        assert!(c.get("sess-a", "tools/list", &params).is_some());
        std::thread::sleep(Duration::from_millis(60));
        assert!(c.get("sess-a", "tools/list", &params).is_none());
    }

    #[test]
    fn private_scope_isolates_sessions() {
        let c = ListResultCache::new();
        let params = json!({});
        let resp = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "tools": [{"name":"a"}], "ttlMs": 60_000, "cacheScope": "private" }
        });
        c.put("sess-a", "tools/list", &params, &resp);
        assert!(c.get("sess-a", "tools/list", &params).is_some());
        assert!(c.get("sess-b", "tools/list", &params).is_none());
    }

    #[test]
    fn public_scope_shared_across_sessions() {
        let c = ListResultCache::new();
        let params = json!({});
        let resp = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "tools": [{"name":"a"}], "ttlMs": 60_000, "cacheScope": "public" }
        });
        c.put("sess-a", "tools/list", &params, &resp);
        assert!(c.get("sess-b", "tools/list", &params).is_some());
    }

    #[test]
    fn absent_ttl_does_not_cache() {
        let c = ListResultCache::new();
        let params = json!({});
        let resp = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "tools": [] }
        });
        c.put("s", "tools/list", &params, &resp);
        assert!(c.get("s", "tools/list", &params).is_none());
    }

    #[test]
    fn list_changed_invalidates() {
        let c = ListResultCache::new();
        let params = json!({});
        let resp = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "tools": [], "ttlMs": 60_000, "cacheScope": "public" }
        });
        c.put("s", "tools/list", &params, &resp);
        c.invalidate_method("tools/list");
        assert!(c.get("s", "tools/list", &params).is_none());
    }
}
