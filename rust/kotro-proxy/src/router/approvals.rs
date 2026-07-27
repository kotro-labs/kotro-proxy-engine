//! Short-lived approval grants for `ask`-class tool calls.
//!
//! A grant is keyed by server + tool + normalized-arguments hash and
//! optionally scoped to one session. Grants expire (default 5 minutes) and
//! are granted only through the authenticated control API (or the `approve`
//! CLI, which uses it).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

pub const DEFAULT_TTL: Duration = Duration::from_secs(300);
const MAX_TTL: Duration = Duration::from_secs(3600);
/// Pending ask requests older than this are dropped from the queue.
const PENDING_TTL: Duration = Duration::from_secs(1800);
const MAX_PENDING: usize = 200;

/// An ask-class call that was blocked for lack of a grant — surfaced to the
/// approval UX (VS Code extension, CLI) so a human can approve it.
#[derive(Clone, Serialize)]
pub struct PendingApproval {
    pub server: String,
    pub tool: String,
    pub args_hash: String,
    pub session: String,
    pub reason: String,
    /// RFC 3339 first-seen timestamp.
    pub at: String,
    #[serde(skip)]
    seen: Instant,
}

#[derive(Default)]
pub struct ApprovalStore {
    grants: Mutex<HashMap<String, Instant>>,
    pending: Mutex<HashMap<String, PendingApproval>>,
}

fn key(server: &str, tool: &str, args_hash: &str, session: &str) -> String {
    format!("{server}\u{1f}{tool}\u{1f}{args_hash}\u{1f}{session}")
}

impl ApprovalStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record (or refresh) a blocked ask request awaiting human approval.
    pub fn note_pending(&self, server: &str, tool: &str, args_hash: &str, session: &str, reason: &str) {
        let k = key(server, tool, args_hash, session);
        let mut pending = self.pending.lock();
        let now = Instant::now();
        pending.retain(|_, p| now.duration_since(p.seen) < PENDING_TTL);
        pending
            .entry(k)
            .and_modify(|p| p.seen = now)
            .or_insert_with(|| PendingApproval {
                server: server.into(),
                tool: tool.into(),
                args_hash: args_hash.into(),
                session: session.into(),
                reason: reason.into(),
                at: crate::flight_recorder::now_rfc3339(),
                seen: now,
            });
        // Bound the queue: drop the oldest entries if oversized.
        if pending.len() > MAX_PENDING {
            let mut entries: Vec<(String, Instant)> =
                pending.iter().map(|(k, p)| (k.clone(), p.seen)).collect();
            entries.sort_by_key(|(_, seen)| *seen);
            for (k, _) in entries.into_iter().take(pending.len() - MAX_PENDING) {
                pending.remove(&k);
            }
        }
    }

    /// Snapshot of unexpired pending approvals, newest first.
    pub fn pending(&self) -> Vec<PendingApproval> {
        let now = Instant::now();
        let mut out: Vec<PendingApproval> = self
            .pending
            .lock()
            .values()
            .filter(|p| now.duration_since(p.seen) < PENDING_TTL)
            .cloned()
            .collect();
        out.sort_by(|a, b| b.seen.cmp(&a.seen));
        out
    }

    /// Record a grant. Empty `session` = any session. TTL is clamped to 1 hour.
    pub fn grant(&self, server: &str, tool: &str, args_hash: &str, session: &str, ttl: Duration) {
        let ttl = ttl.min(MAX_TTL).max(Duration::from_millis(1));
        let mut grants = self.grants.lock();
        let now = Instant::now();
        grants.retain(|_, exp| *exp > now);
        grants.insert(key(server, tool, args_hash, session), now + ttl);
        drop(grants);
        // Clear any matching pending entries (session-scoped and session-wide).
        let mut pending = self.pending.lock();
        pending.remove(&key(server, tool, args_hash, session));
        pending.remove(&key(server, tool, args_hash, ""));
    }

    /// True when an unexpired grant exists for this exact call shape, either
    /// session-scoped or session-wide.
    pub fn check(&self, server: &str, tool: &str, args_hash: &str, session: &str) -> bool {
        let now = Instant::now();
        let grants = self.grants.lock();
        let live = |k: &str| grants.get(k).map(|exp| *exp > now).unwrap_or(false);
        live(&key(server, tool, args_hash, session)) || live(&key(server, tool, args_hash, ""))
    }

    pub fn len(&self) -> usize {
        let now = Instant::now();
        self.grants.lock().values().filter(|exp| **exp > now).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_and_check_exact_shape() {
        let store = ApprovalStore::new();
        assert!(!store.check("files", "delete_file", "abc", "s1"));
        store.grant("files", "delete_file", "abc", "", DEFAULT_TTL);
        // Session-wide grant matches any session.
        assert!(store.check("files", "delete_file", "abc", "s1"));
        assert!(store.check("files", "delete_file", "abc", "s2"));
        // Different args hash does not match.
        assert!(!store.check("files", "delete_file", "other", "s1"));
        // Different tool does not match.
        assert!(!store.check("files", "move_file", "abc", "s1"));
    }

    #[test]
    fn session_scoped_grant_only_matches_that_session() {
        let store = ApprovalStore::new();
        store.grant("files", "delete_file", "abc", "s1", DEFAULT_TTL);
        assert!(store.check("files", "delete_file", "abc", "s1"));
        assert!(!store.check("files", "delete_file", "abc", "s2"));
    }

    #[test]
    fn pending_recorded_and_cleared_on_grant() {
        let store = ApprovalStore::new();
        store.note_pending("files", "delete_file", "abc", "s1", "destructive default ask");
        store.note_pending("files", "delete_file", "abc", "s1", "dup — refresh only");
        assert_eq!(store.pending().len(), 1);
        // A different call is a separate pending item.
        store.note_pending("web", "http_post", "xyz", "s1", "network ask");
        assert_eq!(store.pending().len(), 2);
        // Granting clears the matching pending entry.
        store.grant("files", "delete_file", "abc", "s1", DEFAULT_TTL);
        let pend = store.pending();
        assert_eq!(pend.len(), 1);
        assert_eq!(pend[0].tool, "http_post");
    }

    #[test]
    fn grants_expire() {
        let store = ApprovalStore::new();
        store.grant("files", "delete_file", "abc", "", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(20));
        assert!(!store.check("files", "delete_file", "abc", "s1"));
    }
}
