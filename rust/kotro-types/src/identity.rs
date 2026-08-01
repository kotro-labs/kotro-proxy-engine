//! Principal, agent, and task identity stubs for Phase 0.
//!
//! Phase 1 adds cryptographic binding (signed TaskEnvelope). Phase 0 only
//! requires stable string identifiers on every action event.

use serde::{Deserialize, Serialize};

/// Opaque task identifier. Empty means "no task envelope yet" (legacy sessions).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct TaskId(pub String);

impl TaskId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Principal {
    /// Subject identifier (email, OIDC `sub`, local username, …).
    #[serde(default)]
    pub subject: String,
    /// Optional issuer URL / authority.
    #[serde(default)]
    pub issuer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentIdentity {
    /// Agent product name (claude-code, codex, cursor, …).
    #[serde(default)]
    pub name: String,
    /// Instance / workstation / process label.
    #[serde(default)]
    pub instance: String,
    /// Optional SPIFFE / workload identity URI.
    #[serde(default)]
    pub workload_identity: String,
}
