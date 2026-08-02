//! Process- and request-scoped identity for flight events and approvals.
//!
//! Values come from environment (mcp-wrap / hooks) and optional inbound
//! headers / MCP `_meta` (LLM plane). Empty strings mean "unknown" — the same
//! sentinel TaskEnvelope absence uses until C6 wires verified envelopes.

use serde_json::Value;

/// Flat identity bag copied onto [`crate::flight_recorder::FlightDraft`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityContext {
    pub task_id: String,
    pub parent_task_id: String,
    pub principal_subject: String,
    pub principal_issuer: String,
    pub agent_name: String,
    pub agent_instance: String,
}

impl IdentityContext {
    /// Read identity from process environment.
    ///
    /// Recognized vars: `KOTRO_TASK_ID`, `KOTRO_PARENT_TASK_ID`,
    /// `KOTRO_PRINCIPAL_SUBJECT` (alias `KOTRO_PRINCIPAL`),
    /// `KOTRO_PRINCIPAL_ISSUER`, `KOTRO_AGENT_NAME`, `KOTRO_AGENT_INSTANCE`.
    pub fn from_env() -> Self {
        fn env(name: &str) -> String {
            std::env::var(name).unwrap_or_default().trim().to_string()
        }
        let principal = env("KOTRO_PRINCIPAL_SUBJECT");
        Self {
            task_id: env("KOTRO_TASK_ID"),
            parent_task_id: env("KOTRO_PARENT_TASK_ID"),
            principal_subject: if principal.is_empty() {
                env("KOTRO_PRINCIPAL")
            } else {
                principal
            },
            principal_issuer: env("KOTRO_PRINCIPAL_ISSUER"),
            agent_name: env("KOTRO_AGENT_NAME"),
            agent_instance: env("KOTRO_AGENT_INSTANCE"),
        }
    }

    /// Overlay non-empty values from common HTTP headers.
    pub fn merge_headers(&mut self, headers: &axum::http::HeaderMap) {
        fn hdr(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        }
        if let Some(v) = hdr(headers, "x-kotro-task-id") {
            self.task_id = v;
        }
        if let Some(v) = hdr(headers, "x-kotro-parent-task-id") {
            self.parent_task_id = v;
        }
        if let Some(v) = hdr(headers, "x-kotro-principal") {
            self.principal_subject = v;
        }
        if let Some(v) = hdr(headers, "x-kotro-principal-issuer") {
            self.principal_issuer = v;
        }
        if let Some(v) = hdr(headers, "x-kotro-agent") {
            self.agent_name = v;
        }
        if let Some(v) = hdr(headers, "x-kotro-agent-instance") {
            self.agent_instance = v;
        }
    }

    /// Overlay identity keys from MCP `params._meta` when present.
    pub fn merge_mcp_meta(&mut self, meta: &Value) {
        fn meta_str(meta: &Value, keys: &[&str]) -> Option<String> {
            for k in keys {
                if let Some(s) = meta.get(*k).and_then(Value::as_str) {
                    let t = s.trim();
                    if !t.is_empty() {
                        return Some(t.to_string());
                    }
                }
            }
            None
        }
        if let Some(v) = meta_str(meta, &["kotro.dev/taskId", "task_id", "taskId"]) {
            self.task_id = v;
        }
        if let Some(v) = meta_str(
            meta,
            &["kotro.dev/parentTaskId", "parent_task_id", "parentTaskId"],
        ) {
            self.parent_task_id = v;
        }
        if let Some(v) = meta_str(
            meta,
            &["kotro.dev/principal", "principal", "principal_subject"],
        ) {
            self.principal_subject = v;
        }
        if let Some(v) = meta_str(
            meta,
            &["kotro.dev/principalIssuer", "principal_issuer"],
        ) {
            self.principal_issuer = v;
        }
        if let Some(v) = meta_str(meta, &["kotro.dev/agent", "agent", "agent_name"]) {
            self.agent_name = v;
        }
        if let Some(v) = meta_str(
            meta,
            &["kotro.dev/agentInstance", "agent_instance"],
        ) {
            self.agent_instance = v;
        }
    }

    /// Apply onto a flight draft (only non-empty fields overwrite).
    pub fn apply_to_draft(&self, draft: &mut crate::flight_recorder::FlightDraft) {
        if !self.task_id.is_empty() {
            draft.task_id = self.task_id.clone();
        }
        if !self.parent_task_id.is_empty() {
            draft.parent_task_id = self.parent_task_id.clone();
        }
        if !self.principal_subject.is_empty() {
            draft.principal_subject = self.principal_subject.clone();
        }
        if !self.principal_issuer.is_empty() {
            draft.principal_issuer = self.principal_issuer.clone();
        }
        if !self.agent_name.is_empty() {
            draft.agent_name = self.agent_name.clone();
        }
        if !self.agent_instance.is_empty() {
            draft.agent_instance = self.agent_instance.clone();
        }
    }

    /// JSON object fragment for mcp-wrap / hook reporter payloads.
    pub fn to_report_fields(&self) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        m.insert("task_id".into(), Value::String(self.task_id.clone()));
        m.insert(
            "parent_task_id".into(),
            Value::String(self.parent_task_id.clone()),
        );
        m.insert(
            "principal_subject".into(),
            Value::String(self.principal_subject.clone()),
        );
        m.insert(
            "principal_issuer".into(),
            Value::String(self.principal_issuer.clone()),
        );
        m.insert("agent_name".into(), Value::String(self.agent_name.clone()));
        m.insert(
            "agent_instance".into(),
            Value::String(self.agent_instance.clone()),
        );
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_meta_prefers_kotro_keys() {
        let mut id = IdentityContext::default();
        id.merge_mcp_meta(&json!({
            "kotro.dev/taskId": "t-1",
            "task_id": "ignored",
            "kotro.dev/principal": "alice@example.com",
            "kotro.dev/agent": "claude-code",
        }));
        assert_eq!(id.task_id, "t-1");
        assert_eq!(id.principal_subject, "alice@example.com");
        assert_eq!(id.agent_name, "claude-code");
    }
}
