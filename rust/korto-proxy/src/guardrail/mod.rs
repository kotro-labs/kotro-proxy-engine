//! PII redaction — mirrors `internal/guardrail/redactor.go` (Phase 2.4).

pub mod redaction_map;
pub mod redactor;

pub use redaction_map::{restore_payload, restore_payload_counted, RedactionMap};

pub use redactor::{redact_chat_request, redact_messages_request};
