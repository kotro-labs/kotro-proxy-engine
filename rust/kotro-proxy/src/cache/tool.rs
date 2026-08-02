//! Per-session MCP tool call result cache.
//!
//! Caches tool call results that flow through the proxy as `role: "tool"` messages
//! (OpenAI) or `type: "tool_result"` content blocks (Anthropic). Results are keyed
//! by `(scope, tool_name, SHA-256(canonicalized_args))`.
//!
//! ## TTL policy (SEP-2549)
//!
//! When a server declares `ttlMs` / `cacheScope` on a cacheable result, those hints
//! take precedence via [`ToolCache::put_with_hints`]. Absent hints (LLM-plane tool
//! messages, legacy servers) fall back to name-inferred category TTLs below.
//!
//! `cacheScope: "private"` requires a session-/auth-scoped `scope` key — shared
//! gateways must not reuse a public key for private results.
//!
//! ## What is saved
//!
//! **Token cost (immediate):** when the same tool result appears multiple times inside
//! a single request's message window, [`ToolCache::deduplicate_tool_results`] replaces
//! subsequent occurrences with a compact sentinel, reducing tokens sent to the LLM
//! without altering the first (authoritative) occurrence.
//!
//! **Latency (future):** the cached entries are also the foundation for an MCP proxy
//! mode where Kotro intercepts tool call requests before they reach the MCP server and
//! returns cached results directly.
//!
//! ## Tool category TTLs (fallback when `ttlMs` absent)
//!
//! | Category | Env var | Default | Examples |
//! |----------|---------|---------|---------|
//! | read | `KOTRO_TOOL_CACHE_READ_TTL_SECS` | 30s | `read_file`, `get_file_contents` |
//! | status | `KOTRO_TOOL_CACHE_STATUS_TTL_SECS` | 300s | `git_status`, `list_directory` |
//! | search | `KOTRO_TOOL_CACHE_SEARCH_TTL_SECS` | 3600s | `search_files`, `grep` |
//! | default | — | 60s | everything else |
//!
//! Write operations (`write_file`, `create_file`, `delete_file`, `run_command`)
//! immediately invalidate all read-category entries for the same path in the session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// MCP SEP-2549 `cacheScope` — who may reuse a cached response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheScope {
    /// Safe to share across users / authorization contexts.
    #[default]
    Public,
    /// Bound to one authorization context; must not cross sessions/tokens.
    Private,
}

impl CacheScope {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("private") => Self::Private,
            _ => Self::Public,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

/// Server-declared freshness hints on cacheable MCP results (`ttlMs`, `cacheScope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheHints {
    /// Milliseconds of freshness. `None` means the field was absent (legacy server).
    pub ttl_ms: Option<i64>,
    pub cache_scope: CacheScope,
}

impl CacheHints {
    /// Parse hints from an MCP result object (not the full JSON-RPC envelope).
    pub fn from_result(result: &serde_json::Value) -> Self {
        let ttl_ms = result.get("ttlMs").and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|u| u as i64))
                .or_else(|| v.as_f64().map(|f| f as i64))
        });
        let cache_scope = CacheScope::parse(result.get("cacheScope").and_then(|v| v.as_str()));
        Self {
            ttl_ms,
            cache_scope,
        }
    }

    /// Resolve a concrete storage TTL.
    ///
    /// - `ttl_ms <= 0` or negative → do not cache (`None`)
    /// - `ttl_ms > 0` → that duration
    /// - absent + `honor_absent_as_stale` → do not cache (spec default for modern clients)
    /// - absent otherwise → `fallback` (name-inferred category TTL for LLM-plane path)
    pub fn resolve_ttl(
        &self,
        fallback: Duration,
        honor_absent_as_stale: bool,
    ) -> Option<Duration> {
        match self.ttl_ms {
            Some(ms) if ms <= 0 => None,
            Some(ms) => Some(Duration::from_millis(ms as u64)),
            None if honor_absent_as_stale => None,
            None => Some(fallback),
        }
    }
}

/// Cached result for a single tool call.
#[derive(Clone, Debug)]
pub struct ToolEntry {
    /// The tool function name (e.g. `"read_file"`).
    pub tool_name: String,
    /// The canonicalized arguments JSON passed to the tool.
    pub args_json: String,
    /// The raw result content returned by the tool.
    pub content: String,
    /// When this entry was stored (for age-based metrics, not eviction).
    pub cached_at: Instant,
    /// Server-declared cache scope when known.
    pub cache_scope: CacheScope,
}

/// Tool category determines the applicable TTL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// Short-lived filesystem read results.
    Read,
    /// Medium-lived status / directory listing results.
    Status,
    /// Long-lived search results.
    Search,
    /// Default for unclassified tools.
    Default,
}

impl ToolCategory {
    /// Classify a tool name into a category.
    pub fn from_name(name: &str) -> Self {
        let n = name.to_lowercase();
        if n.contains("write")
            || n.contains("create")
            || n.contains("delete")
            || n.contains("remove")
            || n.contains("run_command")
            || n.contains("execute")
        {
            // Write operations bypass the read cache (handled separately).
            ToolCategory::Default
        } else if n.contains("read")
            || n.contains("get_file")
            || n.contains("fetch_file")
            || n.contains("open_file")
        {
            ToolCategory::Read
        } else if n.contains("status")
            || n.contains("list")
            || n.contains("ls")
            || n.contains("dir")
            || n.contains("tree")
        {
            ToolCategory::Status
        } else if n.contains("search")
            || n.contains("grep")
            || n.contains("find")
            || n.contains("query")
        {
            ToolCategory::Search
        } else {
            ToolCategory::Default
        }
    }

    /// Returns `true` for write operations that should trigger cache invalidation.
    pub fn is_write(name: &str) -> bool {
        let n = name.to_lowercase();
        n.contains("write")
            || n.contains("create_file")
            || n.contains("delete_file")
            || n.contains("remove_file")
            || n.contains("move_file")
            || n.contains("rename_file")
    }
}

/// TTL settings for the tool cache.
#[derive(Clone, Debug)]
pub struct ToolCacheTtls {
    pub read: Duration,
    pub status: Duration,
    pub search: Duration,
    pub default: Duration,
}

impl Default for ToolCacheTtls {
    fn default() -> Self {
        Self {
            read: Duration::from_secs(30),
            status: Duration::from_secs(300),
            search: Duration::from_secs(3600),
            default: Duration::from_secs(60),
        }
    }
}

impl ToolCacheTtls {
    pub fn for_category(&self, cat: ToolCategory) -> Duration {
        match cat {
            ToolCategory::Read => self.read,
            ToolCategory::Status => self.status,
            ToolCategory::Search => self.search,
            ToolCategory::Default => self.default,
        }
    }
}

/// Internal storage: a map from cache key → (entry, expires_at).
type Store = Arc<Mutex<HashMap<String, (ToolEntry, Instant)>>>;

/// Per-session in-memory tool result cache.
///
/// Cheap to clone — the internal store is `Arc`-wrapped.
#[derive(Clone)]
pub struct ToolCache {
    store: Store,
    pub ttls: ToolCacheTtls,
    pub enabled: bool,
}

impl ToolCache {
    pub fn new(enabled: bool, ttls: ToolCacheTtls) -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
            ttls,
            enabled,
        }
    }

    /// A no-op cache (disabled, never stores or returns anything).
    pub fn disabled() -> Self {
        Self::new(false, ToolCacheTtls::default())
    }

    /// Compute the cache key for a given scope, tool name, and arguments JSON.
    ///
    /// Arguments are sorted by key before hashing so `{b:1,a:2}` and `{a:2,b:1}`
    /// produce the same key.
    pub fn cache_key(scope: &str, tool_name: &str, args_json: &str) -> String {
        let canonical_args = canonicalize_args(args_json);
        let mut hasher = Sha256::new();
        hasher.update(scope.as_bytes());
        hasher.update(b":");
        hasher.update(tool_name.as_bytes());
        hasher.update(b":");
        hasher.update(canonical_args.as_bytes());
        let hash = hasher.finalize();
        format!("tool:{:x}", hash)
    }

    /// Store a tool result using name-inferred category TTL (no server hints).
    /// Returns `false` when the cache is disabled.
    pub fn put(&self, scope: &str, tool_name: &str, args_json: &str, content: &str) -> bool {
        self.put_with_hints(scope, tool_name, args_json, content, None)
    }

    /// Store a tool result, preferring server-declared [`CacheHints`] when present.
    ///
    /// For `cacheScope: "private"`, `scope` MUST identify the authorization
    /// context (session / token hash). Returns `false` when disabled, when
    /// `ttlMs` is 0/negative, or when private scope is paired with an empty
    /// `scope` key (refuses to store a shareable entry by mistake).
    pub fn put_with_hints(
        &self,
        scope: &str,
        tool_name: &str,
        args_json: &str,
        content: &str,
        hints: Option<CacheHints>,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        let cat = ToolCategory::from_name(tool_name);
        let fallback = self.ttls.for_category(cat);
        let (ttl, cache_scope) = match hints {
            Some(h) => {
                if h.cache_scope == CacheScope::Private && scope.is_empty() {
                    return false;
                }
                match h.resolve_ttl(fallback, true) {
                    Some(d) => (d, h.cache_scope),
                    None => return false,
                }
            }
            None => (fallback, CacheScope::Private),
        };
        let key = Self::cache_key(scope, tool_name, args_json);
        let canonical = canonicalize_args(args_json);
        let entry = ToolEntry {
            tool_name: tool_name.to_string(),
            args_json: canonical,
            content: content.to_string(),
            cached_at: Instant::now(),
            cache_scope,
        };
        let expires_at = Instant::now() + ttl;
        if let Ok(mut guard) = self.store.lock() {
            guard.insert(key, (entry, expires_at));
        }
        true
    }

    /// Retrieve a cached tool result. Returns `None` when expired or not found.
    pub fn get(&self, scope: &str, tool_name: &str, args_json: &str) -> Option<ToolEntry> {
        if !self.enabled {
            return None;
        }
        let key = Self::cache_key(scope, tool_name, args_json);
        let now = Instant::now();
        if let Ok(mut guard) = self.store.lock() {
            if let Some((entry, expires_at)) = guard.get(&key) {
                if now < *expires_at {
                    return Some(entry.clone());
                }
                // Expired — remove it.
                guard.remove(&key);
            }
        }
        None
    }

    /// Invalidate all read-category entries whose args contain `path`.
    ///
    /// Called when a write operation is detected for the same path so subsequent
    /// reads get fresh content rather than stale cached data.
    pub fn invalidate_by_path(&self, path: &str) {
        if !self.enabled {
            return;
        }
        if let Ok(mut guard) = self.store.lock() {
            guard.retain(|_key, (entry, _expires)| {
                // Evict read-category entries whose args reference the invalidated path.
                ToolCategory::from_name(&entry.tool_name) != ToolCategory::Read
                    || !entry.args_json.contains(path)
            });
        }
    }

    /// Count live (non-expired) entries in the cache.
    pub fn live_count(&self) -> usize {
        let now = Instant::now();
        if let Ok(guard) = self.store.lock() {
            guard.values().filter(|(_, exp)| now < *exp).count()
        } else {
            0
        }
    }

    /// Sentinel string inserted in place of a duplicate tool result to save tokens.
    ///
    /// Must be recognizable by the LLM as "already seen above" — phrased as a
    /// natural parenthetical rather than a raw hash.
    pub fn duplicate_sentinel(tool_name: &str) -> String {
        format!("[Tool result omitted — same `{tool_name}` output already appears earlier in this context]")
    }
}

/// Sort JSON object keys so argument order doesn't affect cache keys.
///
/// Falls back to the original string if parsing fails (e.g. args is a plain string).
fn canonicalize_args(args_json: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(args_json) {
        if let Some(obj) = val.as_object() {
            let sorted: std::collections::BTreeMap<_, _> = obj.iter().collect();
            return serde_json::to_string(&sorted).unwrap_or_else(|_| args_json.to_string());
        }
    }
    args_json.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> ToolCache {
        ToolCache::new(true, ToolCacheTtls::default())
    }

    // ── ToolCategory detection ────────────────────────────────────────────────

    #[test]
    fn classifies_read_tools() {
        assert_eq!(ToolCategory::from_name("read_file"), ToolCategory::Read);
        assert_eq!(ToolCategory::from_name("get_file_contents"), ToolCategory::Read);
    }

    #[test]
    fn classifies_status_tools() {
        assert_eq!(ToolCategory::from_name("git_status"), ToolCategory::Status);
        assert_eq!(ToolCategory::from_name("list_directory"), ToolCategory::Status);
    }

    #[test]
    fn classifies_search_tools() {
        assert_eq!(ToolCategory::from_name("search_files"), ToolCategory::Search);
        assert_eq!(ToolCategory::from_name("grep_codebase"), ToolCategory::Search);
    }

    #[test]
    fn is_write_detects_write_ops() {
        assert!(ToolCategory::is_write("write_file"));
        assert!(ToolCategory::is_write("delete_file"));
        assert!(!ToolCategory::is_write("read_file"));
    }

    // ── put / get ─────────────────────────────────────────────────────────────

    #[test]
    fn stores_and_retrieves_entry() {
        let c = cache();
        c.put("scope-a", "read_file", r#"{"path":"src/main.rs"}"#, "fn main() {}");
        let hit = c.get("scope-a", "read_file", r#"{"path":"src/main.rs"}"#).unwrap();
        assert_eq!(hit.content, "fn main() {}");
        assert_eq!(hit.tool_name, "read_file");
    }

    #[test]
    fn scopes_are_isolated() {
        let c = cache();
        c.put("scope-a", "read_file", r#"{"path":"x.rs"}"#, "content-a");
        assert!(c.get("scope-b", "read_file", r#"{"path":"x.rs"}"#).is_none());
    }

    #[test]
    fn returns_none_for_missing_key() {
        let c = cache();
        assert!(c.get("s", "read_file", r#"{"path":"missing.rs"}"#).is_none());
    }

    #[test]
    fn expired_entry_returns_none() {
        let c = ToolCache::new(
            true,
            ToolCacheTtls {
                read: Duration::from_millis(1),
                ..Default::default()
            },
        );
        c.put("s", "read_file", r#"{"path":"f.rs"}"#, "content");
        std::thread::sleep(Duration::from_millis(10));
        assert!(c.get("s", "read_file", r#"{"path":"f.rs"}"#).is_none());
    }

    #[test]
    fn disabled_cache_never_stores() {
        let c = ToolCache::disabled();
        assert!(!c.put("s", "read_file", r#"{"path":"f.rs"}"#, "content"));
        assert!(c.get("s", "read_file", r#"{"path":"f.rs"}"#).is_none());
    }

    // ── arg canonicalization ──────────────────────────────────────────────────

    #[test]
    fn same_args_different_key_order_hit() {
        let c = cache();
        c.put("s", "read_file", r#"{"path":"f.rs","encoding":"utf8"}"#, "data");
        // Different key order → should still hit.
        let hit = c.get("s", "read_file", r#"{"encoding":"utf8","path":"f.rs"}"#);
        assert!(hit.is_some(), "arg order should not affect cache key");
    }

    // ── write invalidation ────────────────────────────────────────────────────

    #[test]
    fn write_invalidates_read_for_same_path() {
        let c = cache();
        c.put("s", "read_file", r#"{"path":"src/main.rs"}"#, "old content");
        c.invalidate_by_path("src/main.rs");
        assert!(c.get("s", "read_file", r#"{"path":"src/main.rs"}"#).is_none());
    }

    #[test]
    fn write_does_not_evict_different_path() {
        let c = cache();
        c.put("s", "read_file", r#"{"path":"src/lib.rs"}"#, "lib content");
        c.invalidate_by_path("src/main.rs");
        assert!(c.get("s", "read_file", r#"{"path":"src/lib.rs"}"#).is_some());
    }

    // ── live_count ────────────────────────────────────────────────────────────

    #[test]
    fn live_count_tracks_entries() {
        let c = cache();
        assert_eq!(c.live_count(), 0);
        c.put("s", "read_file", r#"{"path":"a.rs"}"#, "a");
        c.put("s", "read_file", r#"{"path":"b.rs"}"#, "b");
        assert_eq!(c.live_count(), 2);
    }

    // ── SEP-2549 hints ────────────────────────────────────────────────────────

    #[test]
    fn server_ttl_overrides_category() {
        let c = ToolCache::new(
            true,
            ToolCacheTtls {
                read: Duration::from_secs(30),
                ..Default::default()
            },
        );
        let hints = CacheHints {
            ttl_ms: Some(5),
            cache_scope: CacheScope::Private,
        };
        assert!(c.put_with_hints("s", "read_file", r#"{"path":"f.rs"}"#, "x", Some(hints)));
        assert!(c.get("s", "read_file", r#"{"path":"f.rs"}"#).is_some());
        std::thread::sleep(Duration::from_millis(15));
        assert!(c.get("s", "read_file", r#"{"path":"f.rs"}"#).is_none());
    }

    #[test]
    fn ttl_zero_skips_store() {
        let c = cache();
        let hints = CacheHints {
            ttl_ms: Some(0),
            cache_scope: CacheScope::Public,
        };
        assert!(!c.put_with_hints("s", "read_file", r#"{"path":"f.rs"}"#, "x", Some(hints)));
        assert!(c.get("s", "read_file", r#"{"path":"f.rs"}"#).is_none());
    }

    #[test]
    fn private_scope_rejects_empty_key() {
        let c = cache();
        let hints = CacheHints {
            ttl_ms: Some(60_000),
            cache_scope: CacheScope::Private,
        };
        assert!(!c.put_with_hints("", "read_file", r#"{}"#, "x", Some(hints)));
    }

    #[test]
    fn parses_cache_hints_from_result() {
        let v = serde_json::json!({"tools": [], "ttlMs": 300000, "cacheScope": "private"});
        let h = CacheHints::from_result(&v);
        assert_eq!(h.ttl_ms, Some(300_000));
        assert_eq!(h.cache_scope, CacheScope::Private);
        assert_eq!(CacheHints::from_result(&serde_json::json!({})).cache_scope, CacheScope::Public);
    }
}
