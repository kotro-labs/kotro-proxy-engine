//! `receipt verify --trust` — R0.4 stub (full signed land receipts are R3).

use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReceiptVerifyError {
    #[error("receipt verify: signed land receipts land in R3 — refusing stub-as-valid")]
    NotImplemented,
    #[error("trust store missing: {0}")]
    TrustMissing(String),
    #[error("receipt file missing: {0}")]
    ReceiptMissing(String),
}

/// Fail-closed stub: never reports a receipt as trusted until R3 wiring exists.
pub fn verify_receipt_stub(receipt: &Path, trust: &Path) -> Result<(), ReceiptVerifyError> {
    if !trust.exists() {
        return Err(ReceiptVerifyError::TrustMissing(
            trust.display().to_string(),
        ));
    }
    if !receipt.exists() {
        return Err(ReceiptVerifyError::ReceiptMissing(
            receipt.display().to_string(),
        ));
    }
    Err(ReceiptVerifyError::NotImplemented)
}
