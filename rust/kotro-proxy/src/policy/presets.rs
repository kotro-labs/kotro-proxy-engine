//! Built-in policy presets: `observe`, `developer`, `locked-down`.

use std::collections::BTreeMap;

use super::{Action, PolicyFile, Rule, ToolClass};

fn defaults(pairs: &[(&str, Action)]) -> BTreeMap<String, Action> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

/// Baseline deny rules shared by `developer` and `locked-down`: credential
/// paths and known-bad exfil shapes are never allowed silently.
fn baseline_denies() -> Vec<Rule> {
    vec![
        Rule {
            id: "deny-ssh-keys".into(),
            action: Some(Action::Deny),
            path: Some("**/.ssh/**".into()),
            reason: Some("SSH private key material must never enter agent context".into()),
            ..Default::default()
        },
        Rule {
            id: "deny-aws-credentials".into(),
            action: Some(Action::Deny),
            path: Some("**/.aws/credentials".into()),
            reason: Some("cloud credentials".into()),
            ..Default::default()
        },
        Rule {
            id: "deny-gnupg".into(),
            action: Some(Action::Deny),
            path: Some("**/.gnupg/**".into()),
            reason: Some("GPG keyring".into()),
            ..Default::default()
        },
        Rule {
            id: "deny-netrc".into(),
            action: Some(Action::Deny),
            path: Some("**/.netrc".into()),
            reason: Some("stored machine credentials".into()),
            ..Default::default()
        },
        Rule {
            id: "trifecta-network-after-sensitive-read".into(),
            action: Some(Action::Deny),
            class: Some(ToolClass::Network),
            data_label: Some("sensitive_read".into()),
            reason: Some(
                "lethal trifecta: session already read sensitive data after untrusted \
                 content — blocking network egress"
                    .into(),
            ),
            ..Default::default()
        },
        Rule {
            id: "deny-drifted-tools".into(),
            action: Some(Action::Deny),
            data_label: Some("tool_drift".into()),
            reason: Some("tool metadata changed after approval (rug-pull quarantine)".into()),
            ..Default::default()
        },
    ]
}

/// Record everything, block nothing. For building a baseline picture.
pub fn observe() -> PolicyFile {
    PolicyFile {
        version: 1,
        preset: "observe".into(),
        defaults: defaults(&[
            ("read_only", Action::Allow),
            ("write", Action::Allow),
            ("destructive", Action::Allow),
            ("credential", Action::Allow),
            ("network", Action::Allow),
            ("exec", Action::Allow),
            ("unknown", Action::Allow),
        ]),
        rules: vec![],
    }
}

/// Sensible daily-driver: reads flow, writes flow, dangerous classes ask,
/// credential paths deny.
pub fn developer() -> PolicyFile {
    PolicyFile {
        version: 1,
        preset: "developer".into(),
        defaults: defaults(&[
            ("read_only", Action::Allow),
            ("write", Action::Allow),
            ("destructive", Action::Ask),
            ("credential", Action::Ask),
            ("network", Action::Allow),
            ("exec", Action::Ask),
            ("unknown", Action::Ask),
        ]),
        rules: baseline_denies(),
    }
}

/// Everything not explicitly allowed is denied or requires approval.
pub fn locked_down() -> PolicyFile {
    PolicyFile {
        version: 1,
        preset: "locked-down".into(),
        defaults: defaults(&[
            ("read_only", Action::Ask),
            ("write", Action::Ask),
            ("destructive", Action::Deny),
            ("credential", Action::Deny),
            ("network", Action::Deny),
            ("exec", Action::Deny),
            ("unknown", Action::Deny),
        ]),
        rules: baseline_denies(),
    }
}

pub fn by_name(name: &str) -> Option<PolicyFile> {
    match name {
        "observe" => Some(observe()),
        "developer" => Some(developer()),
        "locked-down" | "locked_down" => Some(locked_down()),
        _ => None,
    }
}
