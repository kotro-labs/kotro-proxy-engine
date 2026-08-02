//! Stable contracts for the Kotro Agent Transaction Firewall.
//!
//! Phase 0 shipped events/decisions. C5/C6 add bounded schema validation
//! (see `kotro-schema`) and signed TaskEnvelope v1alpha1 authority.

pub mod decision;
pub mod envelope;
pub mod event;
pub mod identity;
pub mod mode;
pub mod reason;
pub mod trust;
pub mod verify;

pub use decision::{
    Decision, DecisionId, DecisionRequest, InterventionPoint, Obligation, RequestedAction, Verdict,
};
pub use envelope::{
    envelope_digest, key_id_for_public_key, signing_input, AgentScope, Budgets, Capabilities,
    TaskEnvelope, API_VERSION as ENVELOPE_API_VERSION, KIND as ENVELOPE_KIND,
};
pub use event::{EventKind, KotroEvent, KotroEventV1, EVENT_SCHEMA_VERSION};
pub use identity::{AgentIdentity, Principal, TaskId};
pub use mode::EnforcementMode;
pub use reason::TaskReason;
pub use trust::{MemoryParentStore, ParentStore, TrustKey, TrustStore};
pub use verify::{
    check_non_expansion, parse_envelope_bytes, public_key_b64, sign_envelope, verify,
    VerificationContext, VerifiedAuthority,
};
