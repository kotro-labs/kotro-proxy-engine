//! Permit product plane — fail-closed `run --permit` (R0.4/R2-A) + acceptance suite (R0.3).
//!
//! Contrasts with MCP `TaskGate` (fail-open unless required): this path always
//! verifies a **v1alpha2** envelope, claims the one-shot ledger only when
//! sandbox launch is committed, and never falls back to host agent execution.

pub mod apply;
pub mod broker;
pub mod dataplane;
pub mod docker;
pub mod http_broker;
pub mod ledger;
pub mod receipt;
pub mod run;
pub mod sandbox;
pub mod stage;
pub mod suite;
pub mod token;

pub use apply::{apply_review_diff, ApplyError, ApplyOptions, ApplyResult};
pub use broker::{
    artifact_hash_of_diff, handle_draft_pr, load_session, write_session, ArtifactRef, BrokerError,
    BrokerOptions, BrokerSession, DraftPrRequest, DraftPrResponse,
};
pub use ledger::{LedgerError, PermitLedger, PermitLedgerState};
pub use receipt::{
    build_draft_pr_receipt, issue_land_receipt, load_or_create_mediator_key, verify_land_receipt,
    verify_receipt_stub, LandReceipt, ReceiptVerifyError, ReceiptVerifyLevels,
    LAND_RECEIPT_MEDIATOR,
};
pub use run::{
    claim_for_sandbox_launch, prepare_run, run_permit, PreparedRun, RunPermitError,
    RunPermitOptions, RunPermitOutcome,
};
pub use sandbox::{sandbox_backend_available, SandboxStatus};
pub use suite::{suite_registry, SuiteCase, SuiteLayer};
pub use token::{
    consume_run_token_for_land, mint_run_token, mint_run_token_attenuated, verify_run_token,
    verify_run_token_for_scope, RunToken,
};
