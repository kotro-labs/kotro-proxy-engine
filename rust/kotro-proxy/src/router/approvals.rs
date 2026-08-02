//! Short-lived approval grants for `ask`-class tool calls.
//!
//! A grant is keyed by server + tool + full JCS argument hash + task id +
//! optional schema digest + optional session (C2 exact-action binding).
//! Grants expire (default 5 minutes) and are granted only through the
//! authenticated control API (or the `approve` CLI, which uses it).

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
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub schema_digest: String,
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

/// Exact-action grant key. Empty `task_id` / `schema_digest` / `session` are
/// significant (legacy-wide grants), not wildcards within a non-empty field.
fn key(
    server: &str,
    tool: &str,
    args_hash: &str,
    task_id: &str,
    schema_digest: &str,
    session: &str,
) -> String {
    format!(
        "{server}\u{1f}{tool}\u{1f}{args_hash}\u{1f}{task_id}\u{1f}{schema_digest}\u{1f}{session}"
    )
}

impl ApprovalStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record (or refresh) a blocked ask request awaiting human approval.
    pub fn note_pending(
        &self,
        server: &str,
        tool: &str,
        args_hash: &str,
        task_id: &str,
        schema_digest: &str,
        session: &str,
        reason: &str,
    ) {
        let k = key(server, tool, args_hash, task_id, schema_digest, session);
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
                task_id: task_id.into(),
                schema_digest: schema_digest.into(),
                session: session.into(),
                reason: reason.into(),
                at: crate::flight_recorder::now_rfc3339(),
                seen: now,
            });
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
    pub fn grant(
        &self,
        server: &str,
        tool: &str,
        args_hash: &str,
        task_id: &str,
        schema_digest: &str,
        session: &str,
        ttl: Duration,
    ) {
        let ttl = ttl.min(MAX_TTL).max(Duration::from_millis(1));
        let mut grants = self.grants.lock();
        let now = Instant::now();
        grants.retain(|_, exp| *exp > now);
        grants.insert(
            key(server, tool, args_hash, task_id, schema_digest, session),
            now + ttl,
        );
        drop(grants);
        let mut pending = self.pending.lock();
        pending.remove(&key(
            server,
            tool,
            args_hash,
            task_id,
            schema_digest,
            session,
        ));
        pending.remove(&key(server, tool, args_hash, task_id, schema_digest, ""));
    }

    /// True when an unexpired grant exists for this exact call shape, either
    /// session-scoped or session-wide (empty session on the grant).
    pub fn check(
        &self,
        server: &str,
        tool: &str,
        args_hash: &str,
        task_id: &str,
        schema_digest: &str,
        session: &str,
    ) -> bool {
        let now = Instant::now();
        let grants = self.grants.lock();
        let live = |k: &str| grants.get(k).map(|exp| *exp > now).unwrap_or(false);
        live(&key(
            server,
            tool,
            args_hash,
            task_id,
            schema_digest,
            session,
        )) || live(&key(
            server,
            tool,
            args_hash,
            task_id,
            schema_digest,
            "",
        ))
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
    fn task_id_isolates_grants() {
        let s = ApprovalStore::new();
        let hash = "sha256:abc";
        s.grant("files", "rm", hash, "task-a", "", "s1", Duration::from_secs(60));
        assert!(s.check("files", "rm", hash, "task-a", "", "s1"));
        assert!(!s.check("files", "rm", hash, "task-b", "", "s1"));
        assert!(!s.check("files", "rm", hash, "", "", "s1"));
    }

    #[test]
    fn full_args_hash_required() {
        let s = ApprovalStore::new();
        s.grant(
            "files",
            "rm",
            "sha256:deadbeef",
            "t",
            "",
            "",
            Duration::from_secs(60),
        );
        assert!(!s.check("files", "rm", "deadbeef", "t", "", ""));
        assert!(s.check("files", "rm", "sha256:deadbeef", "t", "", ""));
    }

    #[test]
    fn schema_digest_binds_exact_action() {
        let s = ApprovalStore::new();
        let hash = "sha256:abc";
        s.grant(
            "files",
            "rm",
            hash,
            "t",
            "sha256:schema1",
            "",
            Duration::from_secs(60),
        );
        assert!(s.check("files", "rm", hash, "t", "sha256:schema1", ""));
        assert!(!s.check("files", "rm", hash, "t", "sha256:schema2", ""));
    }
}
