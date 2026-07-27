//! `kotro doctor` — local agent posture inventory.
//!
//! Discovers agent/MCP configurations on this machine (Claude Code, Cursor,
//! Cline, Continue, plain MCP JSON), scans skill/instruction files as agent
//! supply-chain inputs, produces risk findings, and emits a local Agent Bill
//! of Materials. Read-only: never modifies or uploads anything.

pub mod discovery;
pub mod pins;
pub mod skills;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warn,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Critical => "CRIT",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    /// Stable machine-readable code, e.g. `MCP_INLINE_SECRET`.
    pub code: String,
    /// Server name or file path the finding applies to.
    pub subject: String,
    pub detail: String,
}

/// One MCP server entry from a client configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpServerRecord {
    pub name: String,
    /// claude-code | cursor | cline | continue | mcp-json
    pub client: String,
    pub source_path: String,
    /// stdio | http | sse
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: String,
    /// Environment variable *names* only — values are never recorded.
    pub env_keys: Vec<String>,
    pub executable_path: String,
    /// SHA-256 (16 hex chars) of the resolved executable, when resolvable.
    pub executable_digest: String,
    /// SHA-256 (16 hex chars) of the normalized server config (pin target).
    pub config_digest: String,
    /// Already routed through `kotro-proxy mcp-wrap`.
    pub protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRecord {
    pub path: String,
    /// skill | instructions | hook | rule
    pub kind: String,
}

/// The Agent Bill of Materials + findings for this machine/workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostureReport {
    pub generated_at: String,
    pub workspace: String,
    pub servers: Vec<McpServerRecord>,
    pub skills: Vec<SkillRecord>,
    pub findings: Vec<Finding>,
}

impl PostureReport {
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut info = 0;
        let mut warn = 0;
        let mut critical = 0;
        for f in &self.findings {
            match f.severity {
                Severity::Info => info += 1,
                Severity::Warn => warn += 1,
                Severity::Critical => critical += 1,
            }
        }
        (info, warn, critical)
    }
}

/// Run the full posture scan for `workspace`, comparing against pinned
/// baselines in `state_dir` (drift detection) when available.
pub fn run_doctor(workspace: &std::path::Path, state_dir: Option<&std::path::Path>) -> PostureReport {
    let mut servers = discovery::discover_mcp_servers(workspace);
    let (skills_found, mut skill_findings) = skills::scan_skills(workspace);

    let mut findings: Vec<Finding> = Vec::new();
    for server in &servers {
        findings.extend(discovery::assess_server(server));
    }
    findings.append(&mut skill_findings);

    if let Some(dir) = state_dir {
        let pin_set = pins::load(dir);
        findings.extend(pins::detect_drift(&pin_set, &servers));
    }

    // Deterministic ordering: severity desc, then subject.
    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.subject.cmp(&b.subject)));
    servers.sort_by(|a, b| a.client.cmp(&b.client).then(a.name.cmp(&b.name)));

    PostureReport {
        generated_at: crate::flight_recorder::now_rfc3339(),
        workspace: workspace.display().to_string(),
        servers,
        skills: skills_found,
        findings,
    }
}

/// Human-readable terminal rendering.
pub fn render_text(report: &PostureReport) -> String {
    let mut out = String::new();
    let (info, warn, critical) = report.counts();
    out.push_str(&format!(
        "kotro doctor — agent posture report ({})\nworkspace: {}\n\n",
        report.generated_at, report.workspace
    ));

    out.push_str(&format!("MCP servers discovered: {}\n", report.servers.len()));
    for s in &report.servers {
        let target = if s.url.is_empty() {
            format!("{} {}", s.command, s.args.join(" "))
        } else {
            s.url.clone()
        };
        out.push_str(&format!(
            "  [{}] {:<24} {:<6} {}{}\n",
            s.client,
            s.name,
            s.transport,
            target.trim(),
            if s.protected { "  (kotro-protected)" } else { "" }
        ));
        if !s.executable_digest.is_empty() {
            out.push_str(&format!("      exec sha256: {}  config: {}\n", s.executable_digest, s.config_digest));
        }
    }

    out.push_str(&format!("\nSkill / instruction inputs: {}\n", report.skills.len()));
    for sk in &report.skills {
        out.push_str(&format!("  [{}] {}\n", sk.kind, sk.path));
    }

    out.push_str(&format!(
        "\nFindings: {} critical, {} warn, {} info\n",
        critical, warn, info
    ));
    for f in &report.findings {
        out.push_str(&format!(
            "  {} {:<24} {} — {}\n",
            f.severity.label(),
            f.code,
            f.subject,
            f.detail
        ));
    }
    if report.findings.is_empty() {
        out.push_str("  no findings — posture looks clean\n");
    }
    out
}

pub(crate) fn short_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}
