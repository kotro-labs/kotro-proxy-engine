//! Stable TaskEnvelope reason codes (S4 / C6).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskReason {
    TaskMissing,
    TaskMalformed,
    TaskSchemaInvalid,
    TaskSignatureInvalid,
    TaskKeyUntrusted,
    TaskKeyRevoked,
    TaskNotYetValid,
    TaskExpired,
    TaskParentMissing,
    TaskParentDigestMismatch,
    TaskCycle,
    TaskDelegationDepth,
    TaskCapabilityExpansion,
    TaskPrincipalMismatch,
    TaskAgentMismatch,
    TaskActionOutOfScope,
    TaskBudgetExhausted,
}

impl TaskReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TaskMissing => "task_missing",
            Self::TaskMalformed => "task_malformed",
            Self::TaskSchemaInvalid => "task_schema_invalid",
            Self::TaskSignatureInvalid => "task_signature_invalid",
            Self::TaskKeyUntrusted => "task_key_untrusted",
            Self::TaskKeyRevoked => "task_key_revoked",
            Self::TaskNotYetValid => "task_not_yet_valid",
            Self::TaskExpired => "task_expired",
            Self::TaskParentMissing => "task_parent_missing",
            Self::TaskParentDigestMismatch => "task_parent_digest_mismatch",
            Self::TaskCycle => "task_cycle",
            Self::TaskDelegationDepth => "task_delegation_depth",
            Self::TaskCapabilityExpansion => "task_capability_expansion",
            Self::TaskPrincipalMismatch => "task_principal_mismatch",
            Self::TaskAgentMismatch => "task_agent_mismatch",
            Self::TaskActionOutOfScope => "task_action_out_of_scope",
            Self::TaskBudgetExhausted => "task_budget_exhausted",
        }
    }
}

impl std::fmt::Display for TaskReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
