//! Registered Permit acceptance suite (R0.3).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuiteLayer {
    /// Pure Rust / kotro-types unit coverage.
    Unit,
    /// Invokes `kotro-proxy run --permit` gates.
    Cli,
    /// Shell staging / spike harness.
    Shell,
    /// Evidence reused from prior spikes (not re-executed here).
    SpikeEvidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuiteCase {
    pub id: &'static str,
    pub title: &'static str,
    pub layer: SuiteLayer,
    pub rust_filter: Option<&'static str>,
    pub shell: Option<&'static str>,
    pub notes: &'static str,
}

/// Canonical registry — keep in sync with `testdata/permit-suite/registry.json`.
pub fn suite_registry() -> Vec<SuiteCase> {
    vec![
        SuiteCase {
            id: "P-V1A2-ACCEPT",
            title: "run --permit accepts v1alpha2 only",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::run::tests::accepts_v1alpha2_only"),
            shell: None,
            notes: "CLI rejects non-v1alpha2 api_version before sandbox",
        },
        SuiteCase {
            id: "P-V1A1-PERMIT-FIELDS",
            title: "v1alpha1 containing Permit fields rejects",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::run::tests::rejects_v1alpha1_with_permit_fields"),
            shell: None,
            notes: "Also covered by kotro-types v1alpha1_rejects_repository_fields",
        },
        SuiteCase {
            id: "P-SIGN-DOMAIN",
            title: "Cross-version signing-domain substitution rejects",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::run::tests::cross_version_signing_domain_rejects"),
            shell: None,
            notes: "Signature over V1ALPHA1 domain must not verify as v1alpha2",
        },
        SuiteCase {
            id: "P-REPO-MUTATE",
            title: "Repository identity/pin/base mutation breaks verification",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::run::tests::repo_base_mutation_breaks_verify"),
            shell: None,
            notes: "Tamper base_sha without re-sign → signature invalid",
        },
        SuiteCase {
            id: "P-LAND-NARROW",
            title: "draft_pr → apply_only delegation succeeds; reverse fails",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::run::tests::land_narrow_ok_widen_fails"),
            shell: None,
            notes: "Non-expansion on land.mode",
        },
        SuiteCase {
            id: "P-REPLAY",
            title: "Replay after consume rejected",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::run::tests::replay_after_consume_fails"),
            shell: None,
            notes: "Suite #26 — claim_for_sandbox_launch only; verify-only/deferred unclaimed",
        },
        SuiteCase {
            id: "P-CONCURRENT",
            title: "Concurrent reserve — exactly one wins",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::ledger::tests::concurrent_reserve_exactly_one_wins"),
            shell: None,
            notes: "Suite #26 concurrent claim",
        },
        SuiteCase {
            id: "P-EXPIRY",
            title: "Active-run / prepare expiry rejects",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::run::tests::active_run_expiry_rejects"),
            shell: None,
            notes: "Suite #27 time window on prepare",
        },
        SuiteCase {
            id: "P-STAGING",
            title: "Unsafe staging output / path traversal rejected",
            layer: SuiteLayer::Shell,
            rust_filter: None,
            shell: Some("spikes/r0.1b-topology/test-stage-safety.sh"),
            notes: "Suite #28 — ../, absolute extras, nested deny, host manifest",
        },
        SuiteCase {
            id: "P-NO-HOST-FALLBACK",
            title: "Sandbox/backend absence never falls back to host execution",
            layer: SuiteLayer::Unit,
            rust_filter: Some("permit::run::tests::sandbox_absent_never_runs_host_agent"),
            shell: None,
            notes: "Also host-fallback env is forbidden",
        },
        SuiteCase {
            id: "P-CONTAIN-4-7",
            title: "Containment #4–#7 (ENOENT / DNS+HTTP / IP)",
            layer: SuiteLayer::SpikeEvidence,
            rust_filter: None,
            shell: Some("spikes/r0.1a-containment/run.sh"),
            notes: "R0.1a PASS evidence; Gate A recruiting asset",
        },
        SuiteCase {
            id: "P-TOPOLOGY-16-25",
            title: "Dual-home #16 + host canary #25 (gateway honesty)",
            layer: SuiteLayer::SpikeEvidence,
            rust_filter: None,
            shell: Some("spikes/r0.1b-topology/run.sh"),
            notes: "R0.1b PASS; gateway L3 exposure recorded",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_required_r03_cases() {
        let ids: Vec<_> = suite_registry().iter().map(|c| c.id).collect();
        for need in [
            "P-V1A2-ACCEPT",
            "P-V1A1-PERMIT-FIELDS",
            "P-SIGN-DOMAIN",
            "P-REPO-MUTATE",
            "P-LAND-NARROW",
            "P-REPLAY",
            "P-CONCURRENT",
            "P-EXPIRY",
            "P-STAGING",
            "P-NO-HOST-FALLBACK",
        ] {
            assert!(ids.contains(&need), "missing suite case {need}");
        }
    }
}
