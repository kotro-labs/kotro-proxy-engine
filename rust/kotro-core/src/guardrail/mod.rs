//! PII and secret redaction for LLM prompt pipelines.
//!
//! [`Redactor`] strips secrets from text before it reaches an upstream provider.
//! [`RedactionMap`] tracks the original values so they can be restored in the
//! response stream.
//!
//! ## Pattern coverage
//!
//! | Category | Examples |
//! |----------|---------|
//! | OpenAI / Anthropic API keys | `sk-…`, `sk-ant-…` |
//! | Generic API / secret / token fields | `api_key=…`, `secret=…`, `token=…` |
//! | Passwords | `password=…`, `passwd=…`, `pwd=…` |
//! | AWS access keys | `AKIA…` |
//! | Database URLs | `postgres://…`, `mysql://…`, `mongodb+srv://…`, `redis://…` |
//! | Email addresses | `user@example.com` |
//!
//! ## Example
//!
//! ```rust
//! use kotro_core::guardrail::{Redactor, RedactionMap};
//!
//! let redactor = Redactor::default();
//! let mut map = RedactionMap::new();
//!
//! let safe = redactor.redact("token=abc123 and email=me@example.com", &mut map);
//! // "token=<REDACTED_SECRET_0> and email=<REDACTED_SECRET_1>"
//!
//! // Later, restore in the response:
//! let restored = map.restore(&safe);
//! assert_eq!(restored, "token=abc123 and email=me@example.com");
//! ```

mod patterns;
mod map;

pub use map::RedactionMap;
pub use patterns::build_patterns;

use regex::Regex;

/// Redacts secrets from text using a compiled set of regex patterns.
///
/// Construct once (pattern compilation is expensive), reuse per request.
pub struct Redactor {
    patterns: Vec<Regex>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    /// Build a [`Redactor`] with the full default pattern set.
    pub fn new() -> Self {
        Self {
            patterns: build_patterns(),
        }
    }

    /// Redact all secret matches in `text`, recording originals in `map`.
    ///
    /// Each matched secret is replaced with a placeholder like
    /// `<REDACTED_SECRET_N>` where N is an incrementing counter stored in `map`.
    pub fn redact(&self, text: &str, map: &mut RedactionMap) -> String {
        let mut result = text.to_string();
        for pattern in &self.patterns {
            result = pattern
                .replace_all(&result, |caps: &regex::Captures| {
                    let secret = caps[0].to_string();
                    map.record(secret)
                })
                .into_owned();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_openai_key() {
        // Modern OpenAI project key format: sk-proj-<alphanum+hyphens, 20+ chars>.
        // The short "abc123XYZ" suffix in the old test didn't clear the 20-char
        // minimum; use a realistic-length value.
        let r = Redactor::new();
        let mut map = RedactionMap::new();
        let key = "sk-proj-abcDEF1234567890xyzABCD";
        let out = r.redact(&format!("Authorization: Bearer {key}"), &mut map);
        assert!(!out.contains(key), "key should have been redacted");
        assert!(out.contains("<REDACTED_"), "placeholder should appear in output");
    }

    #[test]
    fn redacts_anthropic_key() {
        // sk-ant-<alphanum+hyphens, 20+ chars after the "sk-ant-" prefix>.
        // "api03-secret" is only 12 chars; pad to a realistic length.
        let r = Redactor::new();
        let mut map = RedactionMap::new();
        let key = "sk-ant-api03-abcDEF1234567890xyzQRST";
        let out = r.redact(&format!("x-api-key: {key}"), &mut map);
        assert!(!out.contains(key), "Anthropic key should have been redacted");
    }

    #[test]
    fn redacts_postgres_url() {
        let r = Redactor::new();
        let mut map = RedactionMap::new();
        let out = r.redact("DATABASE_URL=postgres://user:pass@host/db", &mut map);
        assert!(!out.contains("postgres://user:pass@host/db"));
    }

    #[test]
    fn redacts_email() {
        let r = Redactor::new();
        let mut map = RedactionMap::new();
        let out = r.redact("contact me at alice@example.com please", &mut map);
        assert!(!out.contains("alice@example.com"));
    }

    #[test]
    fn restore_round_trips() {
        let r = Redactor::new();
        let mut map = RedactionMap::new();
        let original = "token=abc123xyz secret stuff";
        let redacted = r.redact(original, &mut map);
        let restored = map.restore(&redacted);
        assert_eq!(restored, original);
    }

    #[test]
    fn no_secrets_leaves_text_unchanged() {
        let r = Redactor::new();
        let mut map = RedactionMap::new();
        let text = "hello world, no secrets here";
        let out = r.redact(text, &mut map);
        assert_eq!(out, text);
        assert!(map.is_empty());
    }
}
