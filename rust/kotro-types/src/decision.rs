//! Deterministic policy decision contract (PEP/PDP style).

use serde::{Deserialize, Serialize};

use crate::identity::{AgentIdentity, Principal, TaskId};
use crate::mode::EnforcementMode;

/// Where in the agent loop the decision is requested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionPoint {
    PreModel,
    PreToolUse,
    PostToolUse,
    PreMcpCall,
    PreNetwork,
    PreCredential,
    Other(String),
}

/// Opaque decision identifier (hex of content hash or random UUID string).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct DecisionId(pub String);

impl DecisionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Deterministic id from a stable snapshot fingerprint.
    pub fn from_fingerprint(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(bytes);
        Self(hex::encode_from_digest(&digest))
    }
}

mod hex {
    pub fn encode_from_digest(digest: &impl AsRef<[u8]>) -> String {
        digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Allow,
    Deny,
    RequireApproval,
    Transform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Obligation {
    RedactFields,
    UseCredentialBroker,
    RestrictDestination,
    RequireSandbox,
    RecordFullEvidence,
    RequestApproval,
    ReduceBudget,
    TerminateAfterAction,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RequestedAction {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub server: String,
    /// Normalized argument fingerprint (never raw secrets).
    #[serde(default)]
    pub args_hash: String,
}

/// Complete snapshot submitted to the decision point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub intervention: InterventionPoint,
    #[serde(default)]
    pub task_id: TaskId,
    #[serde(default)]
    pub principal: Principal,
    #[serde(default)]
    pub agent: AgentIdentity,
    pub action: RequestedAction,
    #[serde(default)]
    pub provenance: Vec<String>,
    #[serde(default)]
    pub policy_revision: String,
    #[serde(default)]
    pub mode: EnforcementMode,
}

/// Normalized verdict returned by the policy decision point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: DecisionId,
    pub verdict: Verdict,
    #[serde(default)]
    pub reason_code: String,
    #[serde(default)]
    pub explanation: String,
    /// Matched rule id when available.
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub policy_revision: String,
    #[serde(default)]
    pub obligations: Vec<Obligation>,
    /// Whether the host actually blocked (false in audit mode).
    #[serde(default)]
    pub enforced: bool,
}

impl Decision {
    /// Build a decision id from request + verdict + rule fingerprint.
    pub fn assign_id(mut self, request: &DecisionRequest) -> Self {
        let material = serde_json::json!({
            "intervention": request.intervention,
            "task_id": request.task_id,
            "action": request.action,
            "policy_revision": request.policy_revision,
            "verdict": self.verdict,
            "rule_id": self.rule_id,
            "reason_code": self.reason_code,
        });
        let bytes = serde_json::to_vec(&material).unwrap_or_default();
        self.id = DecisionId::from_fingerprint(&bytes);
        self.policy_revision = request.policy_revision.clone();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_id_is_stable() {
        let req = DecisionRequest {
            intervention: InterventionPoint::PreMcpCall,
            task_id: TaskId::new("task-1"),
            principal: Principal::default(),
            agent: AgentIdentity::default(),
            action: RequestedAction {
                kind: "tool".into(),
                name: "http_post".into(),
                server: "mock".into(),
                args_hash: "abc".into(),
            },
            provenance: vec![],
            policy_revision: "rev1".into(),
            mode: EnforcementMode::Enforce,
        };
        let d1 = Decision {
            id: DecisionId::default(),
            verdict: Verdict::Deny,
            reason_code: "policy_deny".into(),
            explanation: "denied".into(),
            rule_id: "deny-network".into(),
            policy_revision: String::new(),
            obligations: vec![],
            enforced: true,
        }
        .assign_id(&req);
        let d2 = Decision {
            id: DecisionId::default(),
            verdict: Verdict::Deny,
            reason_code: "policy_deny".into(),
            explanation: "denied".into(),
            rule_id: "deny-network".into(),
            policy_revision: String::new(),
            obligations: vec![],
            enforced: true,
        }
        .assign_id(&req);
        assert_eq!(d1.id, d2.id);
        assert_eq!(d1.id.as_str().len(), 64);
        assert_eq!(d1.policy_revision, "rev1");
    }
}
