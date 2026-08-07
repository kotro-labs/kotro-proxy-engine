//! Stable contracts for the Kotro Agent Transaction Firewall.
//!
//! Phase 0 shipped events/decisions. C5/C6 add bounded schema validation
//! (see `kotro-schema`) and signed TaskEnvelope authority (v1alpha1 MCP;
//! v1alpha2 Permit with repository + land).

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
    envelope_digest, key_id_for_public_key, signing_domain_for, signing_input, unsigned_value,
    AgentScope, Budgets, Capabilities, Delegation, DelegationSigner, DestinationCapability,
    EnvelopePrincipal, EnvelopeSignature, LandAuthority, LandMode, ParentRef, RepositoryAuthority,
    TaskEnvelope, ToolCapability, API_VERSION, API_VERSION_V1ALPHA1, API_VERSION_V1ALPHA2,
    HARD_MAX_DEPTH, KIND, SIGNING_DOMAIN_V1ALPHA1, SIGNING_DOMAIN_V1ALPHA2,
    API_VERSION as ENVELOPE_API_VERSION, KIND as ENVELOPE_KIND,
};
pub use event::{EventKind, KotroEvent, KotroEventV1, EVENT_SCHEMA_VERSION};
pub use identity::{AgentIdentity, Principal, TaskId};
pub use mode::EnforcementMode;
pub use reason::TaskReason;
pub use trust::{MemoryParentStore, ParentStore, TrustKey, TrustStore};
pub use verify::{
    check_envelope_time_window, check_non_expansion, parse_envelope_bytes, parse_rfc3339,
    public_key_b64, sign_envelope, verify, VerificationContext, VerifiedAuthority,
};
