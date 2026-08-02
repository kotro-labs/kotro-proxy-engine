//! Bounded JSON Schema 2020-12 admission and argument validation (C5 / S3).
//!
//! Kotro never fetches schemas from the network. Unsupported dialects,
//! keywords, formats, and external `$ref` values quarantine the tool rather
//! than being silently ignored.

pub mod admit;
pub mod error;
pub mod jcs;
pub mod keywords;
pub mod limits;
pub mod validate;

pub use admit::admit_schema;
pub use error::{AdmissionOutcome, DecisionReason, SanitizedError, SchemaError};
pub use jcs::{args_hash, canonicalize, short_args_hash};
pub use limits::ResourceLimits;
pub use validate::{
    apply_mode, compile, parse_arguments, parse_json_rejecting_duplicates,
    AdmittedSchema, ValidationResult,
};

/// Compatibility shim matching the old `mcp::schema::validate` signature.
/// Prefer [`AdmittedSchema::validate_value`] for new code.
pub fn validate(args: &serde_json::Value, schema: &serde_json::Value) -> Vec<String> {
    match compile(schema, &ResourceLimits::initial()) {
        Ok(admitted) => {
            let result = admitted.validate_value(args);
            if result.ok {
                Vec::new()
            } else if result.reason == DecisionReason::ValidationUnavailable {
                vec!["validation_unavailable".into()]
            } else {
                result.errors.into_iter().map(|e| e.display()).collect()
            }
        }
        Err(e) => vec![e.to_string()],
    }
}
