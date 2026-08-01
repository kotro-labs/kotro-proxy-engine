//! KotroEvent v1 — canonical cross-plane event schema.

use serde::{Deserialize, Serialize};

use crate::identity::{AgentIdentity, Principal, TaskId};

/// Schema version string embedded in every exported event.
pub const EVENT_SCHEMA_VERSION: &str = "kotro.dev/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Request,
    CacheHit,
    CacheMiss,
    CircuitOpen,
    ToolLoop,
    ToolStorm,
    RateLimit,
    Budget,
    Injection,
    KillSwitch,
    Observe,
    ToolDiscovery,
    ToolCall,
    ToolDenied,
    ToolDrift,
    ChainAlert,
    Approval,
    PostureFinding,
    Decision,
    Other(String),
}

impl EventKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Request => "request",
            Self::CacheHit => "cache_hit",
            Self::CacheMiss => "cache_miss",
            Self::CircuitOpen => "circuit_open",
            Self::ToolLoop => "tool_loop",
            Self::ToolStorm => "tool_storm",
            Self::RateLimit => "rate_limit",
            Self::Budget => "budget",
            Self::Injection => "injection",
            Self::KillSwitch => "kill_switch",
            Self::Observe => "observe",
            Self::ToolDiscovery => "tool_discovery",
            Self::ToolCall => "tool_call",
            Self::ToolDenied => "tool_denied",
            Self::ToolDrift => "tool_drift",
            Self::ChainAlert => "chain_alert",
            Self::Approval => "approval",
            Self::PostureFinding => "posture_finding",
            Self::Decision => "decision",
            Self::Other(s) => s.as_str(),
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "request" => Self::Request,
            "cache_hit" => Self::CacheHit,
            "cache_miss" => Self::CacheMiss,
            "circuit_open" => Self::CircuitOpen,
            "tool_loop" => Self::ToolLoop,
            "tool_storm" => Self::ToolStorm,
            "rate_limit" => Self::RateLimit,
            "budget" => Self::Budget,
            "injection" => Self::Injection,
            "kill_switch" => Self::KillSwitch,
            "observe" => Self::Observe,
            "tool_discovery" => Self::ToolDiscovery,
            "tool_call" => Self::ToolCall,
            "tool_denied" => Self::ToolDenied,
            "tool_drift" => Self::ToolDrift,
            "chain_alert" => Self::ChainAlert,
            "approval" => Self::Approval,
            "posture_finding" => Self::PostureFinding,
            "decision" => Self::Decision,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Canonical Phase 0 event. Compatible with the proxy flight recorder fields
/// plus the new platform identifiers (task, decision, policy revision).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KotroEventV1 {
    /// Always `kotro.dev/v1` for this struct.
    #[serde(default = "default_schema")]
    pub api_version: String,
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub at: String,
    /// "llm" | "mcp" | "hook" | "ops" | "egress" | "credential"
    #[serde(default)]
    pub plane: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub task_id: TaskId,
    #[serde(default)]
    pub parent_task_id: TaskId,
    #[serde(default)]
    pub principal: Principal,
    #[serde(default)]
    pub agent: AgentIdentity,
    #[serde(default)]
    pub decision_id: String,
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub policy_revision: String,
    #[serde(default)]
    pub tool_call_id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub span_id: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub route: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub credential_id: String,
    #[serde(default)]
    pub cache_status: String,
    #[serde(default)]
    pub prompt_hash: String,
    #[serde(default)]
    pub estimated_tokens: u64,
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default)]
    pub redaction_count: u32,
    #[serde(default)]
    pub tool_rounds: u32,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub enforced: bool,
    #[serde(default)]
    pub prev_hash: String,
    #[serde(default)]
    pub hash: String,
}

fn default_schema() -> String {
    EVENT_SCHEMA_VERSION.to_string()
}

/// Alias used in docs and exports.
pub type KotroEvent = KotroEventV1;

impl KotroEventV1 {
    pub fn new_v1() -> Self {
        Self {
            api_version: EVENT_SCHEMA_VERSION.to_string(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let mut e = KotroEventV1::new_v1();
        e.kind = EventKind::ToolDenied.as_str().into();
        e.task_id = TaskId::new("task-1");
        e.decision_id = "dec-1".into();
        e.policy_revision = "abc".into();
        e.rule_id = "deny-x".into();
        let raw = serde_json::to_string(&e).unwrap();
        let back: KotroEventV1 = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.api_version, EVENT_SCHEMA_VERSION);
        assert_eq!(back.task_id.as_str(), "task-1");
        assert_eq!(back.decision_id, "dec-1");
    }

    #[test]
    fn fixture_schema_documents_required_platform_fields() {
        let schema = include_str!("../../../schemas/kotro/event-v1.json");
        for key in [
            "task_id",
            "decision_id",
            "policy_revision",
            "rule_id",
            "destination",
            "credential_id",
        ] {
            assert!(
                schema.contains(&format!("\"{key}\"")),
                "schema missing {key}"
            );
        }
        assert!(schema.contains("kotro.dev/v1"));
    }
}
