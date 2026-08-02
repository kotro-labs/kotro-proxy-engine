//! Client-native enforcement adapter for Claude Code hooks.
//!
//! Claude Code invokes `PreToolUse`/`PostToolUse` hooks as commands, passing a
//! JSON event on stdin. `kotro-proxy hook claude-code` normalizes that event
//! into the same [`ToolCallContext`](crate::policy::ToolCallContext) used by
//! the MCP action plane, evaluates the local policy, checks the multi-plane
//! kill switch and short-lived approval grants, records the decision on the
//! flight recorder, and prints a hook decision back to Claude Code.
//!
//! This covers actions that never cross MCP — `Bash`, `Read`, `Write`,
//! `Edit`, `WebFetch`, etc. — using the client's own stable hook contract
//! rather than brittle UI automation or TLS interception.
//!
//! Decision protocol (PreToolUse): we emit
//! `hookSpecificOutput.permissionDecision` ∈ {allow, deny, ask} with a reason.
//! `deny` blocks the call and feeds the reason back to the model; `ask` defers
//! to Claude Code's interactive permission prompt (timeout there means the
//! human decides — for destructive/credential classes we default to `ask`,
//! never silent allow).

use serde_json::Value;

use crate::graph;
use crate::mcp::report::Reporter;
use crate::policy::{self, Action, ToolCallContext, ToolClass};
use crate::posture::short_sha256;

mod install;
pub use install::{claude_code_hook_status, install_claude_code, uninstall_claude_code};

/// Parsed, plane-agnostic view of a Claude Code hook event.
pub struct HookEvent {
    pub event_name: String,
    pub session: String,
    pub tool: String,
    pub input: Value,
}

impl HookEvent {
    pub fn parse(raw: &Value) -> Self {
        let event_name = raw
            .get("hook_event_name")
            .or_else(|| raw.get("hookEventName"))
            .and_then(Value::as_str)
            .unwrap_or("PreToolUse")
            .to_string();
        let session = raw
            .get("session_id")
            .or_else(|| raw.get("sessionId"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| format!("claude-{s}"))
            .unwrap_or_else(|| "claude-unknown".into());
        let tool = raw
            .get("tool_name")
            .or_else(|| raw.get("toolName"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let input = raw
            .get("tool_input")
            .or_else(|| raw.get("toolInput"))
            .cloned()
            .unwrap_or(Value::Null);
        Self { event_name, session, tool, input }
    }

    fn is_post(&self) -> bool {
        self.event_name.eq_ignore_ascii_case("PostToolUse")
    }
}

/// Classify a Claude Code built-in tool, falling back to name-based MCP
/// classification for `mcp__server__tool` and unknown tools.
pub fn classify_builtin(tool: &str, input: &Value) -> ToolClass {
    match tool {
        "Bash" | "BashOutput" => ToolClass::Exec,
        "Read" | "Grep" | "Glob" | "LS" | "NotebookRead" => ToolClass::ReadOnly,
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "Update" => ToolClass::Write,
        "WebFetch" | "WebSearch" => ToolClass::Network,
        // MCP tools surface as mcp__<server>__<tool>; classify by leaf name.
        _ => {
            let leaf = tool.rsplit("__").next().unwrap_or(tool);
            let class = policy::classify_tool(leaf, None);
            // A file-path-bearing unknown builtin is at least a write risk.
            if matches!(class, ToolClass::Unknown)
                && input.get("file_path").and_then(Value::as_str).is_some()
            {
                ToolClass::Write
            } else {
                class
            }
        }
    }
}

/// Build a policy context from a hook event. Extracts paths, domains, and
/// executables from the tool input in the shapes Claude Code uses.
pub fn context_for(event: &HookEvent) -> ToolCallContext {
    let class = classify_builtin(&event.tool, &event.input);
    let mut ctx = ToolCallContext {
        server: "claude-code".into(),
        tool: event.tool.clone(),
        class,
        ..Default::default()
    };

    // Claude Code input shapes: {command} (Bash), {file_path} (Read/Write/
    // Edit), {url} (WebFetch), {pattern,path} (Grep/Glob). `extract_features`
    // already harvests url hosts, path-like strings, and the Exec command's
    // first token from the input object — so we only add what it can't see:
    // Read/Write use `file_path` (not always absolute) and shell commands
    // embed path/url tokens inside a single string.
    if let Some(fp) = event.input.get("file_path").and_then(Value::as_str) {
        ctx.paths.push(fp.to_string());
    }
    if let Some(cmd) = event.input.get("command").and_then(Value::as_str) {
        for tok in cmd.split_whitespace() {
            if tok.starts_with('/') || tok.starts_with("~/") || tok.starts_with("./") {
                ctx.paths.push(tok.to_string());
            } else if let Some(host) = policy_url_host(tok) {
                ctx.domains.push(host);
            }
        }
    }
    // Generic feature extraction (url hosts, absolute paths, exec first token).
    policy::extract_features(&event.input, &mut ctx);

    ctx.paths.sort();
    ctx.paths.dedup();
    ctx.domains.sort();
    ctx.domains.dedup();
    ctx.executables.sort();
    ctx.executables.dedup();
    ctx
}

fn policy_url_host(s: &str) -> Option<String> {
    let rest = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.split('@').next_back()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_string())
}

/// Signal tokens for the cross-plane graph, derived from the hook context.
fn signal_tokens(ctx: &ToolCallContext, input: &Value) -> String {
    let mut tokens: Vec<&str> = Vec::new();
    match ctx.class {
        ToolClass::Network => {
            tokens.push(graph::UNTRUSTED_WEB);
            tokens.push(graph::NETWORK_EGRESS);
        }
        ToolClass::Destructive => tokens.push(graph::DESTRUCTIVE),
        ToolClass::Credential => tokens.push(graph::CREDENTIAL_INPUT),
        _ => {}
    }
    if ctx.paths.iter().any(|p| graph::is_sensitive_path(p)) {
        tokens.push(graph::SENSITIVE_READ);
    }
    if !graph::scan_secrets(&input.to_string()).is_empty() {
        tokens.push(graph::CREDENTIAL_INPUT);
    }
    tokens.sort_unstable();
    tokens.dedup();
    tokens.join(",")
}

/// Result of evaluating one hook event.
pub struct HookOutcome {
    pub decision: Action,
    pub reason: String,
    pub rule_id: String,
    pub args_hash: String,
}

/// Evaluate a hook event against local policy + proxy governance state.
pub async fn evaluate(
    event: &HookEvent,
    workspace: &std::path::Path,
    state_dir: &std::path::Path,
) -> HookOutcome {
    let reporter = Reporter::new(state_dir, event.session.clone());
    let ctx = context_for(event);
    let provenance = signal_tokens(&ctx, &event.input);
    let identity = crate::identity_ctx::IdentityContext::from_env();
    let args_hash = kotro_schema::args_hash(&event.input).unwrap_or_else(|_| {
        format!("sha256:{}", short_sha256(event.input.to_string().as_bytes()))
    });

    // PostToolUse: observe only (scan output for secrets, feed the graph).
    if event.is_post() {
        let mut prov = provenance.clone();
        let output = event
            .input
            .get("tool_response")
            .map(|v| v.to_string())
            .unwrap_or_default();
        if !graph::scan_secrets(&output).is_empty() {
            if !prov.is_empty() {
                prov.push(',');
            }
            prov.push_str(graph::SECRET_OUTPUT);
        }
        let mut draft = serde_json::json!({
            "plane": "hook",
            "kind": "tool_call",
            "server": "claude-code",
            "tool_name": event.tool,
            "route": "post-tool-use",
            "detail": format!("post-tool-use observed ({})", ctx.class.as_str()),
            "enforced": false,
            "provenance": prov,
        });
        if let Some(obj) = draft.as_object_mut() {
            obj.extend(identity.to_report_fields());
        }
        reporter.report(draft);
        return HookOutcome {
            decision: Action::Allow,
            reason: String::new(),
            rule_id: "post-observe".into(),
            args_hash,
        };
    }

    // 1. Multi-plane kill switch (tools halted).
    if reporter
        .kill_scope()
        .await
        .map(|s| s.halts_tools())
        .unwrap_or(false)
    {
        report_decision(&reporter, event, "tool_denied", "kill switch engaged (tools halted)", true, &provenance, &identity);
        return HookOutcome {
            decision: Action::Deny,
            reason: "Kotro kill switch engaged (tools halted).".into(),
            rule_id: "kill-switch".into(),
            args_hash,
        };
    }

    // 2. Local policy, enriched with cross-plane session labels.
    let mut ctx = ctx;
    ctx.data_labels = reporter.session_labels().await;
    let engine = match policy::load_policy(Some(workspace), Some(state_dir)) {
        Ok(e) => e,
        Err(_) => policy::PolicyEngine::compile(policy::presets::developer())
            .expect("developer preset compiles"),
    };
    let decision = engine.evaluate(&ctx);

    match decision.action {
        Action::Allow => {
            report_decision(
                &reporter,
                event,
                "tool_call",
                &format!("allowed by {} ({})", decision.rule_id, decision.evidence),
                false,
                &provenance,
                &identity,
            );
            HookOutcome {
                decision: Action::Allow,
                reason: decision.evidence,
                rule_id: decision.rule_id,
                args_hash,
            }
        }
        Action::Deny => {
            report_decision(
                &reporter,
                event,
                "tool_denied",
                &format!("denied by {} ({})", decision.rule_id, decision.evidence),
                true,
                &provenance,
                &identity,
            );
            HookOutcome {
                decision: Action::Deny,
                reason: format!("[Kotro policy] {}", decision.evidence),
                rule_id: decision.rule_id,
                args_hash,
            }
        }
        Action::Ask => {
            // Honor an existing short-lived approval grant.
            if reporter
                .check_approval(
                    "claude-code",
                    &event.tool,
                    &args_hash,
                    &identity.task_id,
                    "",
                    &decision.evidence,
                )
                .await
            {
                report_decision(
                    &reporter,
                    event,
                    "tool_call",
                    &format!("approved grant matched ({})", decision.rule_id),
                    false,
                    &provenance,
                    &identity,
                );
                return HookOutcome {
                    decision: Action::Allow,
                    reason: "approved grant".into(),
                    rule_id: decision.rule_id,
                    args_hash,
                };
            }
            report_decision(
                &reporter,
                event,
                "tool_denied",
                &format!("requires approval ({})", decision.evidence),
                true,
                &provenance,
                &identity,
            );
            HookOutcome {
                decision: Action::Ask,
                reason: format!(
                    "[Kotro] {} — approve with: kotro-proxy approve --server claude-code \
                     --tool {} --args-hash {} --session {}{}",
                    decision.evidence,
                    event.tool,
                    args_hash,
                    event.session,
                    if identity.task_id.is_empty() {
                        String::new()
                    } else {
                        format!(" --task-id {}", identity.task_id)
                    }
                ),
                rule_id: decision.rule_id,
                args_hash,
            }
        }
    }
}

fn report_decision(
    reporter: &Reporter,
    event: &HookEvent,
    kind: &str,
    detail: &str,
    enforced: bool,
    provenance: &str,
    identity: &crate::identity_ctx::IdentityContext,
) {
    let mut draft = serde_json::json!({
        "plane": "hook",
        "kind": kind,
        "server": "claude-code",
        "tool_name": event.tool,
        "route": "pre-tool-use",
        "detail": detail,
        "enforced": enforced,
        "provenance": provenance,
    });
    if let Some(obj) = draft.as_object_mut() {
        obj.extend(identity.to_report_fields());
    }
    reporter.report(draft);
}

/// Render the Claude Code hook JSON response for a decision.
pub fn render_response(event: &HookEvent, outcome: &HookOutcome) -> Value {
    if event.is_post() {
        return serde_json::json!({ "continue": true });
    }
    let permission = match outcome.decision {
        Action::Allow => "allow",
        Action::Deny => "deny",
        Action::Ask => "ask",
    };
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": permission,
            "permissionDecisionReason": outcome.reason,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(tool: &str, input: Value) -> HookEvent {
        HookEvent {
            event_name: "PreToolUse".into(),
            session: "claude-s1".into(),
            tool: tool.into(),
            input,
        }
    }

    #[test]
    fn classifies_claude_builtins() {
        assert_eq!(classify_builtin("Bash", &Value::Null), ToolClass::Exec);
        assert_eq!(classify_builtin("Read", &Value::Null), ToolClass::ReadOnly);
        assert_eq!(classify_builtin("Write", &Value::Null), ToolClass::Write);
        assert_eq!(classify_builtin("WebFetch", &Value::Null), ToolClass::Network);
    }

    #[test]
    fn classifies_mcp_tool_by_leaf_name() {
        assert_eq!(
            classify_builtin("mcp__github__delete_repo", &Value::Null),
            ToolClass::Destructive
        );
    }

    #[test]
    fn extracts_bash_executable_and_paths() {
        let ev = event("Bash", serde_json::json!({"command": "cat /Users/me/.ssh/id_rsa"}));
        let ctx = context_for(&ev);
        assert_eq!(ctx.class, ToolClass::Exec);
        assert_eq!(ctx.executables, vec!["cat"]);
        assert!(ctx.paths.iter().any(|p| p.contains(".ssh")));
    }

    #[test]
    fn extracts_webfetch_domain() {
        let ev = event("WebFetch", serde_json::json!({"url": "https://evil.example/x"}));
        let ctx = context_for(&ev);
        assert_eq!(ctx.class, ToolClass::Network);
        assert_eq!(ctx.domains, vec!["evil.example"]);
    }

    #[test]
    fn sensitive_path_yields_signal_token() {
        let ev = event("Read", serde_json::json!({"file_path": "/home/x/.aws/credentials"}));
        let ctx = context_for(&ev);
        let tokens = signal_tokens(&ctx, &ev.input);
        assert!(tokens.contains(graph::SENSITIVE_READ));
    }

    #[test]
    fn response_shape_is_claude_compatible() {
        let ev = event("Bash", serde_json::json!({"command": "ls"}));
        let outcome = HookOutcome {
            decision: Action::Deny,
            reason: "nope".into(),
            rule_id: "r".into(),
            args_hash: "h".into(),
        };
        let resp = render_response(&ev, &outcome);
        assert_eq!(
            resp["hookSpecificOutput"]["permissionDecision"],
            serde_json::json!("deny")
        );
    }
}
