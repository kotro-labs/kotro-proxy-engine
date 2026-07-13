//! Compiled regex patterns for secret detection.
//!
//! Kept in sync with `rust/kotro-proxy/src/guardrail/redactor.rs` and the
//! Go reference implementation in `internal/guardrail/pattern.go`.

use regex::Regex;

/// Build the default set of secret-detection patterns.
///
/// This list intentionally overlaps rather than being mutually exclusive — a
/// Postgres URL might also match the generic `api_key` pattern if someone puts
/// credentials in a query parameter. Redundant matches are fine; the redaction
/// map deduplicates by placeholder.
pub fn build_patterns() -> Vec<Regex> {
    [
        r"AKIA[0-9A-Z]{16}",
        r#"(?i)(?:password|passwd|pwd)\s*[:=]\s*['"]?[^\s'"]{4,}['"]?"#,
        r#"(?i)(?:api[_-]?key|secret[_-]?key|token)\s*[:=]\s*['"]?[^\s'"]{8,}['"]?"#,
        r"postgres(?:ql)?://[^\s]+",
        r"mysql://[^\s]+",
        r"mongodb(?:\+srv)?://[^\s]+",
        r"redis://[^\s]+",
        r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}",
        // OpenAI keys: legacy "sk-<alphanum>" and modern project keys "sk-proj-<...>"
        // which contain hyphens. The character class must include '-'.
        r"sk-[a-zA-Z0-9\-]{20,}",
        r"sk-ant-[a-zA-Z0-9\-]{20,}",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("hardcoded redaction regex must compile"))
    .collect()
}
