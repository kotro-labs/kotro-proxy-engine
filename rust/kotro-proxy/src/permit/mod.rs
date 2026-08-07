//! Permit product plane — fail-closed `run --permit` (R0.4/R2-A) + acceptance suite (R0.3).
//!
//! Contrasts with MCP `TaskGate` (fail-open unless required): this path always
//! verifies a **v1alpha2** envelope, claims the one-shot ledger only when
//! sandbox launch is committed, and never falls back to host agent execution.

pub mod apply;
pub mod docker;
pub mod ledger;
pub mod receipt;
pub mod run;
pub mod sandbox;
pub mod stage;
pub mod suite;

pub use apply::{apply_review_diff, ApplyError, ApplyOptions, ApplyResult};
pub use ledger::{LedgerError, PermitLedger, PermitLedgerState};
pub use receipt::{verify_receipt_stub, ReceiptVerifyError};
pub use run::{
    claim_for_sandbox_launch, prepare_run, run_permit, PreparedRun, RunPermitError,
    RunPermitOptions, RunPermitOutcome,
};
pub use sandbox::{sandbox_backend_available, SandboxStatus};
pub use suite::{suite_registry, SuiteCase, SuiteLayer};
