//! Sanitized validation / admission errors (no argument values).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A single sanitized validation error — paths and keywords only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SanitizedError {
    pub path: String,
    pub keyword: String,
    pub message: String,
}

impl SanitizedError {
    pub fn new(
        path: impl Into<String>,
        keyword: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            keyword: keyword.into(),
            message: message.into(),
        }
    }

    pub fn display(&self) -> String {
        format!("{}: {} ({})", self.path, self.message, self.keyword)
    }
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("schema_not_object")]
    SchemaNotObject,
    #[error("schema_root_not_object")]
    SchemaRootNotObject,
    #[error("unsupported_dialect:{0}")]
    UnsupportedDialect(String),
    #[error("unsupported_keyword:{0}")]
    UnsupportedKeyword(String),
    #[error("unsupported_format:{0}")]
    UnsupportedFormat(String),
    #[error("external_ref:{0}")]
    ExternalRef(String),
    #[error("limit_exceeded:{0}")]
    LimitExceeded(String),
    #[error("invalid_regex")]
    InvalidRegex,
    #[error("compile_failed:{0}")]
    CompileFailed(String),
    #[error("compile_timeout")]
    CompileTimeout,
    #[error("validation_unavailable")]
    ValidationUnavailable,
    #[error("arguments_oversized")]
    ArgumentsOversized,
    #[error("duplicate_object_key:{0}")]
    DuplicateObjectKey(String),
    #[error("malformed_json:{0}")]
    MalformedJson(String),
}

/// Stable reason codes shared by audit and enforce paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    WouldQuarantine,
    Quarantined,
    WouldDeny,
    Denied,
    ValidationUnavailable,
    InvalidParameters,
    Admitted,
    Valid,
}

#[derive(Debug, Clone)]
pub struct AdmissionOutcome {
    pub admitted: bool,
    pub reason: DecisionReason,
    pub detail: String,
    pub schema_digest: Option<String>,
}
