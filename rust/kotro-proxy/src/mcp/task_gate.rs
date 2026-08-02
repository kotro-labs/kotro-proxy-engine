//! Optional TaskEnvelope gate for mcp-wrap (C6).
//!
//! When `KOTRO_TASK_ENVELOPE` points at a signed envelope and
//! `KOTRO_TRUST_STORE` at an operator trust registry, every `tools/call` is
//! checked against the verified capability set (tool name, optional schema
//! digest, optional exact args hashes) and the envelope budget. Absent
//! configuration is fail-open unless `KOTRO_TASK_REQUIRED=true`.

//! Optional TaskEnvelope gate for mcp-wrap (C6).
//!
//! When `KOTRO_TASK_ENVELOPE` points at a signed envelope and
//! `KOTRO_TRUST_STORE` at an operator trust registry, every `tools/call` is
//! checked against the verified capability set (tool name, optional schema
//! digest, optional exact args hashes) and the envelope budget. Absent
//! configuration is fail-open unless `KOTRO_TASK_REQUIRED=true`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use kotro_types::{
    parse_envelope_bytes, verify, MemoryParentStore, TaskReason, TrustStore, VerificationContext,
    VerifiedAuthority,
};

use crate::flight_recorder::now_rfc3339;
use crate::identity_ctx::IdentityContext as ProxyIdentity;

/// Runtime task authority for one mcp-wrap process.
pub struct TaskGate {
    authority: Option<Arc<VerifiedAuthority>>,
    tool_calls: AtomicU64,
    required: bool,
    audience: Option<String>,
}

impl TaskGate {
    /// Load from environment. Returns an inactive (fail-open) gate when unset.
    pub fn from_env() -> Result<Self, String> {
        let required = matches!(
            std::env::var("KOTRO_TASK_REQUIRED")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "required"
        );
        let envelope_path = std::env::var("KOTRO_TASK_ENVELOPE").ok().filter(|s| !s.is_empty());
        let trust_path = std::env::var("KOTRO_TRUST_STORE").ok().filter(|s| !s.is_empty());
        let audience = std::env::var("KOTRO_TASK_AUDIENCE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let parent_dir = std::env::var("KOTRO_PARENT_STORE_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        match (envelope_path, trust_path) {
            (None, None) if !required => Ok(Self::inactive(required)),
            (None, _) | (_, None) if required => Err(
                "KOTRO_TASK_REQUIRED is set but KOTRO_TASK_ENVELOPE / KOTRO_TRUST_STORE are missing"
                    .into(),
            ),
            (None, _) | (_, None) => Ok(Self::inactive(required)),
            (Some(env_path), Some(trust_path)) => {
                Self::load(Path::new(&env_path), Path::new(&trust_path), audience, parent_dir, required)
            }
        }
    }

    fn inactive(required: bool) -> Self {
        Self {
            authority: None,
            tool_calls: AtomicU64::new(0),
            required,
            audience: None,
        }
    }

    fn load(
        envelope_path: &Path,
        trust_path: &Path,
        audience: Option<String>,
        parent_dir: Option<PathBuf>,
        required: bool,
    ) -> Result<Self, String> {
        let trust = TrustStore::load(trust_path).map_err(|r| format!("trust store: {r}"))?;
        let mut parents = MemoryParentStore::default();
        if let Some(dir) = parent_dir {
            load_parent_dir(&dir, &mut parents)?;
        }
        let raw = std::fs::read(envelope_path)
            .map_err(|e| format!("read task envelope {}: {e}", envelope_path.display()))?;
        let envelope =
            parse_envelope_bytes(&raw).map_err(|r| format!("parse task envelope: {r}"))?;
        let now = now_rfc3339();
        let ctx = VerificationContext {
            trust: &trust,
            parents: &parents,
            now_rfc3339: &now,
            expected_audience: audience.as_deref(),
            kill_engaged: false,
        };
        let authority = verify(&envelope, &ctx).map_err(|r| format!("verify task envelope: {r}"))?;
        Ok(Self {
            authority: Some(Arc::new(authority)),
            tool_calls: AtomicU64::new(0),
            required,
            audience,
        })
    }

    pub fn is_active(&self) -> bool {
        self.authority.is_some()
    }

    /// Identity overlay derived from the verified envelope (if any).
    pub fn identity_overlay(&self) -> ProxyIdentity {
        let Some(auth) = &self.authority else {
            return ProxyIdentity::default();
        };
        let env = &auth.envelope;
        ProxyIdentity {
            task_id: env.task_id.clone(),
            parent_task_id: env
                .parent
                .as_ref()
                .map(|p| p.task_id.clone())
                .unwrap_or_default(),
            principal_subject: env.principal.subject.clone(),
            principal_issuer: env.principal.issuer.clone(),
            agent_name: env
                .agent_scope
                .names
                .first()
                .cloned()
                .unwrap_or_default(),
            agent_instance: String::new(),
        }
    }

    /// Gate one tool call. `Ok` when no envelope is configured (and not required),
    /// or when the call is within the verified capability set and budget.
    pub fn check_tool_call(
        &self,
        server: &str,
        tool: &str,
        args_hash: &str,
        schema_digest: &str,
    ) -> Result<(), TaskReason> {
        let Some(auth) = &self.authority else {
            return if self.required {
                Err(TaskReason::TaskMissing)
            } else {
                Ok(())
            };
        };
        // Re-check expiry against wall clock on every call.
        let now = now_rfc3339();
        if now.as_str() < auth.envelope.not_before.as_str() {
            return Err(TaskReason::TaskNotYetValid);
        }
        if now.as_str() > auth.envelope.expires_at.as_str() {
            return Err(TaskReason::TaskExpired);
        }
        if let Some(aud) = &self.audience {
            if &auth.envelope.audience != aud {
                return Err(TaskReason::TaskActionOutOfScope);
            }
        }
        auth.allows_tool(
            server,
            tool,
            if args_hash.is_empty() {
                None
            } else {
                Some(args_hash)
            },
            if schema_digest.is_empty() {
                None
            } else {
                Some(schema_digest)
            },
        )?;
        if let Some(max) = auth.max_tool_calls() {
            let used = self.tool_calls.fetch_add(1, Ordering::Relaxed) + 1;
            if used > max {
                // Roll back the optimistic increment so the counter stays honest.
                self.tool_calls.fetch_sub(1, Ordering::Relaxed);
                return Err(TaskReason::TaskBudgetExhausted);
            }
        }
        Ok(())
    }
}

fn load_parent_dir(dir: &Path, store: &mut MemoryParentStore) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json")
            && path.extension().and_then(|e| e.to_str()) != Some("yaml")
            && path.extension().and_then(|e| e.to_str()) != Some("yml")
        {
            continue;
        }
        let raw = std::fs::read(&path).map_err(|e| e.to_string())?;
        let env = parse_envelope_bytes(&raw).map_err(|r| format!("{}: {r}", path.display()))?;
        let digest = kotro_types::envelope_digest(&env).map_err(|e| e.to_string())?;
        store.entries.insert(digest, env);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_gate_allows_when_not_required() {
        let g = TaskGate::inactive(false);
        assert!(g
            .check_tool_call("files", "read_file", "sha256:x", "")
            .is_ok());
    }

    #[test]
    fn inactive_required_gate_denies() {
        let g = TaskGate::inactive(true);
        assert_eq!(
            g.check_tool_call("files", "read_file", "sha256:x", "")
                .unwrap_err(),
            TaskReason::TaskMissing
        );
    }
}
