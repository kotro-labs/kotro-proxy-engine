//! Cross-plane session graph and dangerous-chain (lethal trifecta) detection.
//!
//! Every recorded [`FlightEvent`](crate::flight_recorder::FlightEvent) — LLM
//! plane, MCP action plane, hooks, and operator actions — is fed through
//! [`SessionGraph::observe`]. The graph maintains per-session provenance
//! state and emits [`ChainAlert`]s when a *sequence* of events becomes
//! dangerous, not when a single regex matches:
//!
//! - `lethal-trifecta` — untrusted content entered the session, sensitive
//!   data was read, and a network/open-world action follows.
//! - `drift-then-exec` — a tool whose metadata drifted from its pinned
//!   baseline is subsequently invoked.
//! - `credential-egress` — credential material is present in the session and
//!   a network egress action follows.
//! - `destructive-storm` — repeated destructive/non-idempotent calls in a
//!   short window.
//!
//! Signals travel inside the canonical event schema: the `provenance` field
//! carries comma-separated tokens (`untrusted_web`, `sensitive_read`,
//! `network_egress`, `destructive`, `credential_input`, `secret_output`)
//! attached by the plane that observed them (mcp-wrap, hook adapter, or the
//! LLM proxy itself).

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::flight_recorder::{FlightEvent, FlightKind};

/// Provenance/signal tokens understood by the graph.
pub const UNTRUSTED_WEB: &str = "untrusted_web";
pub const UNTRUSTED_REPO: &str = "untrusted_repo";
pub const UNTRUSTED_TOOL: &str = "untrusted_tool";
pub const SENSITIVE_READ: &str = "sensitive_read";
pub const NETWORK_EGRESS: &str = "network_egress";
pub const DESTRUCTIVE: &str = "destructive";
pub const CREDENTIAL_INPUT: &str = "credential_input";
pub const SECRET_OUTPUT: &str = "secret_output";
pub const TOOL_DRIFT: &str = "tool_drift";

const DESTRUCTIVE_STORM_WINDOW: Duration = Duration::from_secs(120);
const DESTRUCTIVE_STORM_THRESHOLD: usize = 3;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChainAlert {
    /// Stable chain-rule identifier (e.g. `lethal-trifecta`).
    pub rule: String,
    pub session: String,
    pub severity: String,
    /// Human-readable evidence: which events formed the chain.
    pub evidence: String,
}

#[derive(Default)]
struct SessionState {
    /// All signal tokens seen in this session.
    labels: HashSet<String>,
    /// Tools quarantined for metadata drift.
    drifted_tools: HashSet<String>,
    /// Timestamps of recent destructive calls (storm detection).
    destructive_at: VecDeque<Instant>,
    /// Chain rules already fired (dedupe — one alert per rule per session).
    fired: HashSet<String>,
    /// Short evidence trail: latest signal-bearing events.
    trail: VecDeque<String>,
}

impl SessionState {
    fn has_untrusted(&self) -> bool {
        self.labels.contains(UNTRUSTED_WEB)
            || self.labels.contains(UNTRUSTED_REPO)
            || self.labels.contains(UNTRUSTED_TOOL)
    }

    fn has_sensitive(&self) -> bool {
        self.labels.contains(SENSITIVE_READ) || self.labels.contains(SECRET_OUTPUT)
    }

    fn push_trail(&mut self, entry: String) {
        self.trail.push_back(entry);
        while self.trail.len() > 12 {
            self.trail.pop_front();
        }
    }
}

#[derive(Default)]
pub struct SessionGraph {
    sessions: Mutex<HashMap<String, SessionState>>,
}

impl SessionGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one recorded event through the correlator. Returns any chain
    /// alerts that this event completed. `ChainAlert` events themselves are
    /// ignored to avoid feedback loops.
    pub fn observe(&self, event: &FlightEvent) -> Vec<ChainAlert> {
        if event.session.is_empty() || matches!(event.kind, FlightKind::ChainAlert) {
            return Vec::new();
        }
        let tokens = parse_tokens(&event.provenance);
        let mut derived: HashSet<String> = tokens.iter().cloned().collect();

        // Kind-derived signals (planes don't have to label these explicitly).
        match event.kind {
            // The injection scanner fired on user/tool content: untrusted
            // content is now in the model context.
            FlightKind::Injection => {
                derived.insert(UNTRUSTED_TOOL.into());
            }
            FlightKind::ToolDrift => {
                derived.insert(TOOL_DRIFT.into());
            }
            _ => {}
        }

        let mut sessions = self.sessions.lock();
        let st = sessions.entry(event.session.clone()).or_default();
        let mut alerts: Vec<ChainAlert> = Vec::new();

        if event.kind == FlightKind::ToolDrift && !event.tool_name.is_empty() {
            st.drifted_tools.insert(event.tool_name.clone());
        }

        if !derived.is_empty() {
            st.push_trail(format!(
                "{} {} {}{} [{}]",
                event.at,
                event.plane,
                event.tool_name,
                if event.tool_name.is_empty() { "" } else { " " },
                derived.iter().cloned().collect::<Vec<_>>().join(",")
            ));
        }

        // R2: drift-then-exec — a quarantined/drifted tool is being invoked.
        if matches!(event.kind, FlightKind::ToolCall | FlightKind::ToolDenied)
            && st.drifted_tools.contains(&event.tool_name)
            && st.fired.insert(format!("drift-then-exec:{}", event.tool_name))
        {
            alerts.push(ChainAlert {
                rule: "drift-then-exec".into(),
                session: event.session.clone(),
                severity: "critical".into(),
                evidence: format!(
                    "tool '{}' metadata drifted from its pinned baseline and was then invoked \
                     (server '{}')",
                    event.tool_name, event.server
                ),
            });
        }

        // R3: destructive storm — repeated destructive calls in a short window.
        if derived.contains(DESTRUCTIVE) {
            let now = Instant::now();
            st.destructive_at.push_back(now);
            while let Some(front) = st.destructive_at.front() {
                if now.duration_since(*front) > DESTRUCTIVE_STORM_WINDOW {
                    st.destructive_at.pop_front();
                } else {
                    break;
                }
            }
            if st.destructive_at.len() >= DESTRUCTIVE_STORM_THRESHOLD
                && st.fired.insert("destructive-storm".into())
            {
                alerts.push(ChainAlert {
                    rule: "destructive-storm".into(),
                    session: event.session.clone(),
                    severity: "warning".into(),
                    evidence: format!(
                        "{} destructive/non-idempotent calls within {}s (latest: '{}')",
                        st.destructive_at.len(),
                        DESTRUCTIVE_STORM_WINDOW.as_secs(),
                        event.tool_name
                    ),
                });
            }
        }

        // Absorb this event's signals into the session state *before*
        // evaluating egress chains so a single event carrying e.g.
        // credential_input + network_egress still completes a chain.
        for t in &derived {
            st.labels.insert(t.clone());
        }

        if derived.contains(NETWORK_EGRESS) {
            // R1: lethal trifecta.
            if st.has_untrusted()
                && st.has_sensitive()
                && st.fired.insert("lethal-trifecta".into())
            {
                alerts.push(ChainAlert {
                    rule: "lethal-trifecta".into(),
                    session: event.session.clone(),
                    severity: "critical".into(),
                    evidence: format!(
                        "untrusted content + sensitive data + network egress via '{}': {}",
                        event.tool_name,
                        st.trail.iter().cloned().collect::<Vec<_>>().join(" → ")
                    ),
                });
            }
            // R4: credential material leaving over the network.
            if st.labels.contains(CREDENTIAL_INPUT)
                && st.fired.insert("credential-egress".into())
            {
                alerts.push(ChainAlert {
                    rule: "credential-egress".into(),
                    session: event.session.clone(),
                    severity: "critical".into(),
                    evidence: format!(
                        "credential-bearing input observed earlier in this session, followed by \
                         network egress via '{}'",
                        event.tool_name
                    ),
                });
            }
        }

        alerts
    }

    /// All raw signal labels seen for a session (dashboard / diagnostics).
    pub fn labels(&self, session: &str) -> Vec<String> {
        let sessions = self.sessions.lock();
        let Some(st) = sessions.get(session) else {
            return Vec::new();
        };
        let mut out: Vec<String> = st.labels.iter().cloned().collect();
        if !st.drifted_tools.is_empty() {
            out.push(TOOL_DRIFT.into());
        }
        out.sort();
        out.dedup();
        out
    }

    /// Labels exported to the policy engine (`data_label` rule matching).
    ///
    /// `sensitive_read` is only exported once *untrusted* content is also in
    /// the session, so the preset trifecta deny rule blocks the full chain
    /// rather than every network call after a routine local read.
    pub fn policy_labels(&self, session: &str) -> Vec<String> {
        let sessions = self.sessions.lock();
        let Some(st) = sessions.get(session) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for label in &st.labels {
            if label == SENSITIVE_READ || label == SECRET_OUTPUT {
                if st.has_untrusted() {
                    out.push(SENSITIVE_READ.into());
                }
            } else {
                out.push(label.clone());
            }
        }
        if !st.drifted_tools.is_empty() {
            out.push(TOOL_DRIFT.into());
        }
        out.sort();
        out.dedup();
        out
    }

    /// Signal-bearing evidence trail for a session (dashboard timeline).
    pub fn trail(&self, session: &str) -> Vec<String> {
        let sessions = self.sessions.lock();
        sessions
            .get(session)
            .map(|st| st.trail.iter().cloned().collect())
            .unwrap_or_default()
    }
}

fn parse_tokens(provenance: &str) -> Vec<String> {
    provenance
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

// ── Signal extraction helpers (used by mcp-wrap and hook adapters) ──────────

/// Substrings that mark a filesystem path as credential/secret-bearing.
const SENSITIVE_PATH_MARKERS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".netrc",
    ".env",
    ".pgpass",
    ".npmrc",
    ".git-credentials",
    "credentials",
    "secret",
    "keychain",
    "id_rsa",
    "id_ed25519",
    "private_key",
    ".kube/config",
];

pub fn is_sensitive_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    SENSITIVE_PATH_MARKERS.iter().any(|m| lower.contains(m))
}

/// Deterministic secret-shape scan (AWS keys, common API tokens, PEM keys).
/// Used on tool inputs and outputs; returns the kinds found.
pub fn scan_secrets(text: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    if find_aws_key(text) {
        found.push("aws-access-key");
    }
    if find_prefixed_token(text, "sk-", 20) {
        found.push("api-secret-key");
    }
    if find_prefixed_token(text, "ghp_", 36) || find_prefixed_token(text, "github_pat_", 22) {
        found.push("github-token");
    }
    if text.contains("xoxb-") || text.contains("xoxp-") {
        found.push("slack-token");
    }
    if text.contains("PRIVATE KEY-----") {
        found.push("pem-private-key");
    }
    found
}

fn find_aws_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    for (i, w) in bytes.windows(4).enumerate() {
        if w == b"AKIA" {
            let tail = &bytes[i + 4..];
            if tail.len() >= 16
                && tail[..16]
                    .iter()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                return true;
            }
        }
    }
    false
}

fn find_prefixed_token(text: &str, prefix: &str, min_tail: usize) -> bool {
    let mut rest = text;
    while let Some(pos) = rest.find(prefix) {
        let tail = &rest[pos + prefix.len()..];
        let run = tail
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
            .count();
        if run >= min_tail {
            return true;
        }
        rest = &rest[pos + prefix.len()..];
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight_recorder::{FlightEvent, FlightKind};

    fn ev(session: &str, kind: FlightKind, tool: &str, provenance: &str) -> FlightEvent {
        FlightEvent {
            session: session.into(),
            kind,
            plane: "mcp".into(),
            tool_name: tool.into(),
            provenance: provenance.into(),
            at: "2026-07-25T00:00:00.000Z".into(),
            ..Default::default()
        }
    }

    #[test]
    fn trifecta_requires_all_three_stages() {
        let g = SessionGraph::new();
        // Sensitive read alone → nothing.
        assert!(g
            .observe(&ev("s", FlightKind::ToolCall, "read_file", SENSITIVE_READ))
            .is_empty());
        // Network egress without untrusted content → still nothing.
        assert!(g
            .observe(&ev("s", FlightKind::ToolCall, "http_post", NETWORK_EGRESS))
            .is_empty());
        // Untrusted content arrives.
        assert!(g
            .observe(&ev("s", FlightKind::ToolCall, "fetch_url", UNTRUSTED_WEB))
            .is_empty());
        // Next egress completes the chain.
        let alerts = g.observe(&ev("s", FlightKind::ToolCall, "http_post", NETWORK_EGRESS));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule, "lethal-trifecta");
        assert_eq!(alerts[0].severity, "critical");
        // Fires once per session.
        assert!(g
            .observe(&ev("s", FlightKind::ToolCall, "http_post", NETWORK_EGRESS))
            .is_empty());
    }

    #[test]
    fn sessions_are_isolated() {
        let g = SessionGraph::new();
        g.observe(&ev("a", FlightKind::ToolCall, "fetch", UNTRUSTED_WEB));
        g.observe(&ev("a", FlightKind::ToolCall, "read", SENSITIVE_READ));
        // Session b has none of session a's history.
        assert!(g
            .observe(&ev("b", FlightKind::ToolCall, "post", NETWORK_EGRESS))
            .is_empty());
    }

    #[test]
    fn injection_event_counts_as_untrusted() {
        let g = SessionGraph::new();
        g.observe(&ev("s", FlightKind::Injection, "", ""));
        g.observe(&ev("s", FlightKind::ToolCall, "read", SENSITIVE_READ));
        let alerts = g.observe(&ev("s", FlightKind::ToolCall, "post", NETWORK_EGRESS));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule, "lethal-trifecta");
    }

    #[test]
    fn drift_then_exec_fires_on_invocation() {
        let g = SessionGraph::new();
        g.observe(&ev("s", FlightKind::ToolDrift, "read_file", ""));
        let alerts = g.observe(&ev("s", FlightKind::ToolDenied, "read_file", ""));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule, "drift-then-exec");
    }

    #[test]
    fn credential_egress_chain() {
        let g = SessionGraph::new();
        g.observe(&ev("s", FlightKind::ToolCall, "run", CREDENTIAL_INPUT));
        let alerts = g.observe(&ev("s", FlightKind::ToolCall, "post", NETWORK_EGRESS));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule, "credential-egress");
    }

    #[test]
    fn single_event_with_credential_and_egress_completes_chain() {
        let g = SessionGraph::new();
        let alerts = g.observe(&ev(
            "s",
            FlightKind::ToolCall,
            "curl",
            "credential_input,network_egress",
        ));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule, "credential-egress");
    }

    #[test]
    fn destructive_storm_after_threshold() {
        let g = SessionGraph::new();
        assert!(g
            .observe(&ev("s", FlightKind::ToolCall, "rm", DESTRUCTIVE))
            .is_empty());
        assert!(g
            .observe(&ev("s", FlightKind::ToolCall, "rm", DESTRUCTIVE))
            .is_empty());
        let alerts = g.observe(&ev("s", FlightKind::ToolCall, "rm", DESTRUCTIVE));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule, "destructive-storm");
    }

    #[test]
    fn policy_labels_gate_sensitive_read_on_untrusted() {
        let g = SessionGraph::new();
        g.observe(&ev("s", FlightKind::ToolCall, "read", SENSITIVE_READ));
        // Without untrusted content, sensitive_read is not exported to policy…
        assert!(!g.policy_labels("s").contains(&SENSITIVE_READ.to_string()));
        assert!(g.labels("s").contains(&SENSITIVE_READ.to_string()));
        // …after untrusted content arrives, it is.
        g.observe(&ev("s", FlightKind::ToolCall, "fetch", UNTRUSTED_WEB));
        assert!(g.policy_labels("s").contains(&SENSITIVE_READ.to_string()));
    }

    #[test]
    fn sensitive_paths_detected() {
        assert!(is_sensitive_path("/Users/me/.ssh/id_rsa"));
        assert!(is_sensitive_path("/home/x/.aws/credentials"));
        assert!(is_sensitive_path("C:\\Users\\x\\.env"));
        assert!(is_sensitive_path("/etc/secrets/db.yaml"));
        assert!(!is_sensitive_path("/tmp/notes.txt"));
        assert!(!is_sensitive_path("src/main.rs"));
    }

    #[test]
    fn secret_scanner_finds_common_shapes() {
        assert_eq!(scan_secrets("key=AKIAIOSFODNN7EXAMPLE"), vec!["aws-access-key"]);
        assert_eq!(
            scan_secrets("token: sk-abcdefghijklmnopqrstuvwx"),
            vec!["api-secret-key"]
        );
        assert_eq!(
            scan_secrets("ghp_0123456789abcdefghijklmnopqrstuvwxyz"),
            vec!["github-token"]
        );
        assert_eq!(
            scan_secrets("-----BEGIN RSA PRIVATE KEY-----"),
            vec!["pem-private-key"]
        );
        assert!(scan_secrets("nothing secret here, sk-short").is_empty());
    }
}
