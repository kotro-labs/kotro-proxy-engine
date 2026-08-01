//! Stable contracts for the Kotro Agent Transaction Firewall (Phase 0).
//!
//! These types are intentionally small and serde-friendly so:
//! - the proxy can embed them in the flight recorder,
//! - adapters (MCP, hooks, Numbat) can share one vocabulary,
//! - JSON Schema fixtures can lock compatibility across minor versions.
//!
//! Phase 1 will add signed `TaskEnvelope` verification; Phase 0 only ships
//! the schema and deterministic decision identifiers.

pub mod decision;
pub mod event;
pub mod identity;
pub mod mode;

pub use decision::{
    Decision, DecisionId, DecisionRequest, InterventionPoint, Obligation, RequestedAction, Verdict,
};
pub use event::{EventKind, KotroEvent, KotroEventV1, EVENT_SCHEMA_VERSION};
pub use identity::{AgentIdentity, Principal, TaskId};
pub use mode::EnforcementMode;
