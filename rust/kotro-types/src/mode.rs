//! Audit vs enforce mode for the control plane.
//!
//! Distinct from per-feature toggles (injection block, budget block). This is
//! the unified rollout dial: disabled skips evaluation; audit/observe never
//! blocks; enforce does.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EnforcementMode {
    /// Skip guardrail evaluation entirely (no scan, no flight events).
    Disabled,
    /// Record decisions and emit evidence; do not block.
    Audit,
    /// Alias of Audit for operators coming from kill-switch observe mode.
    Observe,
    /// Actively deny / halt / revoke according to policy.
    #[default]
    Enforce,
}

impl EnforcementMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "disabled" | "off" | "none" => Self::Disabled,
            "audit" | "observe" | "log" | "warn" => Self::Audit,
            _ => Self::Enforce,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Audit | Self::Observe => "audit",
            Self::Enforce => "enforce",
        }
    }

    /// Whether this mode may block or terminate actions.
    pub fn enforces(self) -> bool {
        matches!(self, Self::Enforce)
    }

    /// Whether guardrails should evaluate (scan / record) at all.
    pub fn evaluates(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_aliases() {
        assert_eq!(EnforcementMode::parse("observe"), EnforcementMode::Audit);
        assert_eq!(EnforcementMode::parse("AUDIT"), EnforcementMode::Audit);
        assert_eq!(EnforcementMode::parse("disabled"), EnforcementMode::Disabled);
        assert_eq!(EnforcementMode::parse("enforce"), EnforcementMode::Enforce);
        assert!(EnforcementMode::Enforce.enforces());
        assert!(!EnforcementMode::Audit.enforces());
        assert!(!EnforcementMode::Disabled.enforces());
        assert!(!EnforcementMode::Disabled.evaluates());
        assert!(EnforcementMode::Audit.evaluates());
    }
}
