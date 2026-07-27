//! `kotro-proxy mcp-wrap` — the governed MCP relay runtime.
//!
//! stdio mode:   client ⇄ (this process) ⇄ child MCP server process
//! HTTP mode:    client ⇄ (this process, stdio) ⇄ remote Streamable HTTP server
//!
//! Interception points:
//! - all inbound methods: kill switch + allowlist (deny unknown methods).
//! - `tools/list` responses: pin metadata, quarantine drift (rug pulls).
//! - `tools/call` requests: quarantine → schema validation →
//!   deny/ask/allow policy (with approval grants and session labels).
//!
//! Known limitation (documented): in HTTP mode, server-initiated messages on
//! a standalone GET stream are not relayed; request/response and SSE-response
//! flows are.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::flight_recorder::KillScope;
use crate::policy::{self, PolicyEngine, ToolCallContext};
use crate::posture::short_sha256;

use super::report::Reporter;
use super::{id_key, parse_message, rpc_error, ERR_INVALID_ARGS, ERR_KILL_SWITCH, ERR_POLICY_DENIED};

pub struct WrapOptions {
    pub name: String,
    /// Child command for stdio mode. Empty when `url` is set.
    pub command: Vec<String>,
    /// Remote Streamable HTTP endpoint.
    pub url: Option<String>,
    pub state_dir: PathBuf,
    pub workspace: PathBuf,
    pub session: String,
}

struct Governance {
    server: String,
    engine: PolicyEngine,
    state_dir: PathBuf,
    reporter: Reporter,
    halted: Arc<AtomicBool>,
    quarantined: Mutex<HashSet<String>>,
    /// Latest pinned input schemas by tool name (from `tools/list`).
    schemas: Mutex<HashMap<String, Value>>,
}

impl Governance {
    fn new(opts: &WrapOptions) -> Result<Self, String> {
        let engine = policy::load_policy(Some(&opts.workspace), Some(&opts.state_dir))?;
        Ok(Self {
            server: opts.name.clone(),
            engine,
            state_dir: opts.state_dir.clone(),
            reporter: Reporter::new(&opts.state_dir, opts.session.clone()),
            halted: Arc::new(AtomicBool::new(false)),
            quarantined: Mutex::new(HashSet::new()),
            schemas: Mutex::new(HashMap::new()),
        })
    }

    fn event(&self, kind: &str, tool: &str, detail: String, enforced: bool) {
        self.event_with(kind, tool, detail, enforced, "");
    }

    fn event_with(&self, kind: &str, tool: &str, detail: String, enforced: bool, provenance: &str) {
        self.reporter.report(serde_json::json!({
            "plane": "mcp",
            "kind": kind,
            "server": self.server,
            "tool_name": tool,
            "route": "mcp-wrap",
            "detail": detail,
            "enforced": enforced,
            "provenance": provenance,
        }));
        // MCP OTel span — tool name + decision only, never arguments.
        let decision = if kind == "tool_denied" {
            if enforced { "deny" } else { "observe" }
        } else {
            "allow"
        };
        crate::telemetry::genai::record_tool_span(
            &self.reporter.session,
            &self.server,
            tool,
            decision,
            "",
        );
    }

    /// Gate one `tools/call`. `Err((code, message))` means block.
    async fn check_tool_call(&self, msg: &Value) -> Result<(), (i64, String)> {
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let tool = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);

        // 1. Multi-plane kill switch.
        if self.halted.load(Ordering::Relaxed) {
            self.event("tool_denied", &tool, "kill switch engaged (tools halted)".into(), true);
            return Err((
                ERR_KILL_SWITCH,
                "[KOTRO KILL SWITCH] tool execution halted by operator".into(),
            ));
        }

        // 2. Rug-pull quarantine.
        if self.quarantined.lock().contains(&tool) {
            self.event(
                "tool_denied",
                &tool,
                "tool metadata drifted from approved baseline (quarantined)".into(),
                true,
            );
            return Err((
                ERR_POLICY_DENIED,
                format!(
                    "[KOTRO QUARANTINE] tool '{tool}' metadata changed after approval. \
                     Review it, then run: kotro-proxy mcp repin --server {}",
                    self.server
                ),
            ));
        }

        // 3. Argument validation against the pinned schema.
        if let Some(schema) = self.schemas.lock().get(&tool).cloned() {
            let violations = super::schema::validate(&args, &schema);
            if !violations.is_empty() {
                self.event(
                    "tool_denied",
                    &tool,
                    format!("schema violations: {}", violations.join(", ")),
                    true,
                );
                return Err((
                    ERR_INVALID_ARGS,
                    format!(
                        "[KOTRO SCHEMA] arguments rejected: {}",
                        violations.join("; ")
                    ),
                ));
            }
        }

        // 4. Policy evaluation with session provenance labels.
        let mut ctx = ToolCallContext {
            server: self.server.clone(),
            tool: tool.clone(),
            class: policy::classify_tool(&tool, None),
            data_labels: self.reporter.session_labels().await,
            ..Default::default()
        };
        policy::extract_features(&args, &mut ctx);
        let decision = self.engine.evaluate(&ctx);

        // Signal tokens for the cross-plane session graph. The graph turns
        // these into provenance labels and chain alerts (lethal trifecta).
        let provenance = signal_tokens(&ctx, &args);

        match decision.action {
            policy::Action::Allow => {
                self.event_with(
                    "tool_call",
                    &tool,
                    format!("allowed by {} ({})", decision.rule_id, decision.evidence),
                    false,
                    &provenance,
                );
                Ok(())
            }
            policy::Action::Deny => {
                self.event_with(
                    "tool_denied",
                    &tool,
                    format!("denied by {} ({})", decision.rule_id, decision.evidence),
                    true,
                    &provenance,
                );
                Err((
                    ERR_POLICY_DENIED,
                    format!(
                        "[KOTRO POLICY] denied by rule '{}': {}",
                        decision.rule_id, decision.evidence
                    ),
                ))
            }
            policy::Action::Ask => {
                let args_hash = short_sha256(args.to_string().as_bytes());
                if self
                    .reporter
                    .check_approval(&self.server, &tool, &args_hash, &decision.evidence)
                    .await
                {
                    self.event_with(
                        "tool_call",
                        &tool,
                        format!("approved grant matched ({})", decision.rule_id),
                        false,
                        &provenance,
                    );
                    return Ok(());
                }
                self.event_with(
                    "tool_denied",
                    &tool,
                    format!(
                        "requires approval ({}, {}) — no grant found",
                        decision.rule_id, decision.evidence
                    ),
                    true,
                    &provenance,
                );
                Err((
                    ERR_POLICY_DENIED,
                    format!(
                        "[KOTRO APPROVAL REQUIRED] '{tool}' needs approval ({}). Grant it with: \
                         kotro-proxy approve --server {} --tool {tool} --args-hash {args_hash}",
                        decision.evidence, self.server
                    ),
                ))
            }
        }
    }

    /// Deterministic secret scan on a tool result. A hit marks the session
    /// as carrying sensitive data (`secret_output`), which arms the
    /// lethal-trifecta chain rule for any later network egress.
    fn scan_tool_output(&self, tool: &str, payload: &str) {
        let kinds = crate::graph::scan_secrets(payload);
        if kinds.is_empty() {
            return;
        }
        self.event_with(
            "tool_call",
            tool,
            format!("secret material detected in tool output: {}", kinds.join(", ")),
            false,
            crate::graph::SECRET_OUTPUT,
        );
    }

    /// Intercept a `tools/list` result: pin, quarantine drift, filter.
    fn process_tools_list_response(&self, response: &mut Value) {
        let Some(tools) = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(Value::as_array)
            .cloned()
        else {
            return;
        };

        let outcome = super::pin::process_tools_list(&self.state_dir, &self.server, &tools);

        for name in &outcome.newly_pinned {
            self.event(
                "tool_discovery",
                name,
                "tool metadata pinned (trust-on-first-use)".into(),
                false,
            );
        }
        for name in &outcome.drifted {
            self.event(
                "tool_drift",
                name,
                "tool metadata drifted from pinned baseline — quarantined".into(),
                true,
            );
        }

        {
            let mut q = self.quarantined.lock();
            for name in &outcome.drifted {
                q.insert(name.clone());
            }
        }
        {
            let mut schemas = self.schemas.lock();
            for tool in &outcome.filtered_tools {
                if let (Some(name), Some(schema)) = (
                    tool.get("name").and_then(Value::as_str),
                    tool.get("inputSchema"),
                ) {
                    schemas.insert(name.to_string(), schema.clone());
                }
            }
        }

        if let Some(result) = response.get_mut("result") {
            result["tools"] = Value::Array(outcome.filtered_tools);
        }
    }
}

/// Derive session-graph signal tokens for one tool call from its policy
/// context and raw arguments. Comma-separated, carried in `provenance`.
fn signal_tokens(ctx: &crate::policy::ToolCallContext, args: &Value) -> String {
    use crate::graph;
    let mut tokens: Vec<&str> = Vec::new();
    match ctx.class {
        // A network/open-world tool both ingests untrusted content and can
        // exfiltrate — pessimistic on both directions.
        crate::policy::ToolClass::Network => {
            tokens.push(graph::UNTRUSTED_WEB);
            tokens.push(graph::NETWORK_EGRESS);
        }
        crate::policy::ToolClass::Destructive => tokens.push(graph::DESTRUCTIVE),
        crate::policy::ToolClass::Credential => tokens.push(graph::CREDENTIAL_INPUT),
        _ => {}
    }
    if ctx.paths.iter().any(|p| graph::is_sensitive_path(p)) {
        tokens.push(graph::SENSITIVE_READ);
    }
    if !graph::scan_secrets(&args.to_string()).is_empty() {
        tokens.push(graph::CREDENTIAL_INPUT);
    }
    tokens.sort_unstable();
    tokens.dedup();
    tokens.join(",")
}


/// MCP client→server methods Kotro will relay. Everything else is denied so
/// wrap cannot be used as an ungoverned tunnel for experimental / sampling
/// methods. `tools/call` still gets full policy; other allowlisted methods
/// still honor the multi-plane kill switch.
fn method_is_allowlisted(method: &str) -> bool {
    matches!(
        method,
        "initialize"
            | "ping"
            | "tools/list"
            | "tools/call"
            | "resources/list"
            | "resources/read"
            | "resources/templates/list"
            | "resources/subscribe"
            | "resources/unsubscribe"
            | "prompts/list"
            | "prompts/get"
            | "completion/complete"
            | "logging/setLevel"
    ) || method.starts_with("notifications/")
}

impl Governance {
    /// Gate every inbound request: kill switch first, then method allowlist.
    /// `tools/call` continues through `check_tool_call` for full policy.
    fn gate_request(&self, method: &str) -> Result<(), (i64, String)> {
        if self.halted.load(Ordering::Relaxed) {
            return Err((
                ERR_KILL_SWITCH,
                "[KOTRO KILL SWITCH] tool execution halted by operator".into(),
            ));
        }
        if method_is_allowlisted(method) {
            return Ok(());
        }
        Err((
            ERR_POLICY_DENIED,
            format!(
                "[KOTRO] MCP method '{method}' is not allowlisted by mcp-wrap;                  only initialize/ping/tools/resources/prompts/completion/logging                  and notifications are relayed"
            ),
        ))
    }
}

/// Background task: poll the proxy kill switch; halt tools and (stdio mode)
/// terminate the managed child when engaged.
fn spawn_kill_switch_poller(
    gov: Arc<Governance>,
    child_pid: Option<u32>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut announced = false;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let scope = gov.reporter.kill_scope().await.unwrap_or(KillScope::None);
            let halt = scope.halts_tools();
            gov.halted.store(halt, Ordering::Relaxed);
            if halt && !announced {
                announced = true;
                gov.event(
                    "kill_switch",
                    "",
                    format!("kill switch (scope {}) reached action plane", scope.as_str()),
                    true,
                );
                if let Some(pid) = child_pid {
                    // Terminate the Kotro-managed MCP child process.
                    #[cfg(unix)]
                    unsafe {
                        libc_kill(pid as i32);
                    }
                }
            } else if !halt {
                announced = false;
            }
        }
    })
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32) {
    // SIGTERM without pulling in the libc crate.
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, 15);
}

pub async fn run_wrap(opts: WrapOptions) -> Result<(), String> {
    let gov = Arc::new(Governance::new(&opts)?);
    gov.event(
        "tool_discovery",
        "",
        format!(
            "mcp-wrap started (policy preset: {}, mode: {})",
            gov.engine.preset(),
            if opts.url.is_some() { "http" } else { "stdio" }
        ),
        false,
    );

    if let Some(url) = opts.url.clone() {
        run_http(gov, url).await
    } else {
        run_stdio(gov, &opts.command).await
    }
}

// ── stdio mode ───────────────────────────────────────────────────────────────

async fn run_stdio(gov: Arc<Governance>, command: &[String]) -> Result<(), String> {
    if command.is_empty() {
        return Err("mcp-wrap: no child command given (use `-- <command> [args…]`)".into());
    }
    let mut child = tokio::process::Command::new(&command[0])
        .args(&command[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("mcp-wrap: failed to spawn '{}': {e}", command[0]))?;

    let child_pid = child.id();
    let mut child_stdin = child.stdin.take().ok_or("child stdin unavailable")?;
    let child_stdout = child.stdout.take().ok_or("child stdout unavailable")?;

    let stdout = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));
    let pending_lists: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    // Forwarded tools/call ids → tool name, for output secret scanning.
    let pending_calls: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let _poller = spawn_kill_switch_poller(gov.clone(), child_pid);

    // Child stdout → our stdout (intercept tools/list results).
    let out_task = {
        let gov = gov.clone();
        let stdout = stdout.clone();
        let pending_lists = pending_lists.clone();
        let pending_calls = pending_calls.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(child_stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let emitted = if let Some(msg) = parse_message(&line) {
                    if let Some(id) = &msg.id {
                        if let Some(tool) = pending_calls.lock().remove(&id_key(id)) {
                            gov.scan_tool_output(&tool, &line);
                        }
                        if pending_lists.lock().remove(&id_key(id)) {
                            let mut modified = msg.raw.clone();
                            gov.process_tools_list_response(&mut modified);
                            Some(modified.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                let out_line = emitted.unwrap_or(line);
                let mut out = stdout.lock().await;
                if out.write_all(out_line.as_bytes()).await.is_err() {
                    break;
                }
                let _ = out.write_all(b"\n").await;
                let _ = out.flush().await;
            }
        })
    };

    // Our stdin (client) → child stdin (govern tools/call).
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut forward = true;
        if let Some(msg) = parse_message(&line) {
            match msg.method.as_deref() {
                Some(method) => {
                    if let Err((code, message)) = gov.gate_request(method) {
                        forward = false;
                        if let Some(id) = &msg.id {
                            let err_line = rpc_error(id, code, &message);
                            let mut out = stdout.lock().await;
                            let _ = out.write_all(err_line.as_bytes()).await;
                            let _ = out.write_all(b"\n").await;
                            let _ = out.flush().await;
                        }
                    } else if method == "tools/call" {
                        if let Err((code, message)) = gov.check_tool_call(&msg.raw).await {
                            forward = false;
                            if let Some(id) = &msg.id {
                                let err_line = rpc_error(id, code, &message);
                                let mut out = stdout.lock().await;
                                let _ = out.write_all(err_line.as_bytes()).await;
                                let _ = out.write_all(b"\n").await;
                                let _ = out.flush().await;
                            }
                        } else if let Some(id) = &msg.id {
                            // Track the forwarded call so the response can be
                            // scanned for leaked secret material.
                            let tool = msg
                                .raw
                                .get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                                .to_string();
                            pending_calls.lock().insert(id_key(id), tool);
                        }
                    } else if method == "tools/list" {
                        if let Some(id) = &msg.id {
                            pending_lists.lock().insert(id_key(id));
                        }
                    }
                }
                None => {}
            }
        }
        if forward {
            if child_stdin.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = child_stdin.write_all(b"\n").await;
            let _ = child_stdin.flush().await;
        }
    }

    // Client closed stdin: shut down the child and drain.
    drop(child_stdin);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
    let _ = child.start_kill();
    let _ = out_task.await;
    Ok(())
}

// ── Streamable HTTP mode ─────────────────────────────────────────────────────

async fn run_http(gov: Arc<Governance>, url: String) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let stdout = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));
    let mut mcp_session: Option<String> = None;

    let _poller = spawn_kill_switch_poller(gov.clone(), None);

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Some(msg) = parse_message(&line) else {
            continue;
        };

        let method = msg.method.as_deref().unwrap_or("");
        let is_tools_list = method == "tools/list";
        let is_tools_call = method == "tools/call";
        let call_tool = msg
            .raw
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        if let Err((code, message)) = gov.gate_request(method) {
            if let Some(id) = &msg.id {
                write_line(&stdout, &rpc_error(id, code, &message)).await;
            }
            continue;
        }
        if is_tools_call {
            if let Err((code, message)) = gov.check_tool_call(&msg.raw).await {
                if let Some(id) = &msg.id {
                    write_line(&stdout, &rpc_error(id, code, &message)).await;
                }
                continue;
            }
        }

        let mut req = client
            .post(&url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(line.clone());
        if let Some(sid) = &mcp_session {
            req = req.header("mcp-session-id", sid.clone());
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                if let Some(id) = &msg.id {
                    write_line(
                        &stdout,
                        &rpc_error(id, -32000, &format!("[KOTRO] upstream MCP unreachable: {e}")),
                    )
                    .await;
                }
                continue;
            }
        };

        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            mcp_session = Some(sid.to_string());
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp.text().await.unwrap_or_default();
        if body.trim().is_empty() {
            continue; // 202 for notifications
        }
        if is_tools_call {
            gov.scan_tool_output(&call_tool, &body);
        }

        if content_type.contains("text/event-stream") {
            for data in body
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(str::trim)
                .filter(|d| !d.is_empty())
            {
                emit_response(&gov, &stdout, data, is_tools_list).await;
            }
        } else {
            emit_response(&gov, &stdout, &body, is_tools_list).await;
        }
    }
    Ok(())
}

async fn emit_response(
    gov: &Arc<Governance>,
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    payload: &str,
    intercept_tools_list: bool,
) {
    if intercept_tools_list {
        if let Ok(mut v) = serde_json::from_str::<Value>(payload) {
            if v.get("result").map(|r| r.get("tools").is_some()).unwrap_or(false) {
                gov.process_tools_list_response(&mut v);
                write_line(stdout, &v.to_string()).await;
                return;
            }
        }
    }
    write_line(stdout, payload).await;
}

async fn write_line(stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>, line: &str) {
    let mut out = stdout.lock().await;
    let _ = out.write_all(line.as_bytes()).await;
    let _ = out.write_all(b"\n").await;
    let _ = out.flush().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap_opts(dir: &std::path::Path) -> WrapOptions {
        WrapOptions {
            name: "files".into(),
            command: vec![],
            url: None,
            state_dir: dir.join("state"),
            workspace: dir.to_path_buf(),
            session: "test-session".into(),
        }
    }

    #[tokio::test]
    async fn denies_quarantined_tool() {
        let dir = tempfile::tempdir().unwrap();
        let gov = Governance::new(&wrap_opts(dir.path())).unwrap();
        gov.quarantined.lock().insert("read_file".into());
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "read_file", "arguments": {"path": "/tmp/x"}}
        });
        let err = gov.check_tool_call(&call).await.unwrap_err();
        assert_eq!(err.0, ERR_POLICY_DENIED);
        assert!(err.1.contains("QUARANTINE"));
    }

    #[tokio::test]
    async fn denies_kill_switch() {
        let dir = tempfile::tempdir().unwrap();
        let gov = Governance::new(&wrap_opts(dir.path())).unwrap();
        gov.halted.store(true, Ordering::Relaxed);
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "read_file", "arguments": {}}
        });
        let err = gov.check_tool_call(&call).await.unwrap_err();
        assert_eq!(err.0, ERR_KILL_SWITCH);
    }

    #[tokio::test]
    async fn denies_credential_path_via_policy() {
        // Default developer preset denies **/.ssh/** paths.
        let dir = tempfile::tempdir().unwrap();
        let gov = Governance::new(&wrap_opts(dir.path())).unwrap();
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "read_file", "arguments": {"path": "/Users/me/.ssh/id_rsa"}}
        });
        let err = gov.check_tool_call(&call).await.unwrap_err();
        assert_eq!(err.0, ERR_POLICY_DENIED);
        assert!(err.1.contains("deny-ssh-keys"), "{}", err.1);
    }

    #[tokio::test]
    async fn allows_plain_read() {
        let dir = tempfile::tempdir().unwrap();
        let gov = Governance::new(&wrap_opts(dir.path())).unwrap();
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "read_file", "arguments": {"path": "/tmp/notes.txt"}}
        });
        assert!(gov.check_tool_call(&call).await.is_ok());
    }

    #[tokio::test]
    async fn schema_validation_blocks_bad_args() {
        let dir = tempfile::tempdir().unwrap();
        let gov = Governance::new(&wrap_opts(dir.path())).unwrap();
        gov.schemas.lock().insert(
            "read_file".into(),
            serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {"path": {"type": "string"}}
            }),
        );
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "read_file", "arguments": {"path": 42}}
        });
        let err = gov.check_tool_call(&call).await.unwrap_err();
        assert_eq!(err.0, ERR_INVALID_ARGS);
    }

    #[tokio::test]
    async fn tools_list_interception_filters_drift() {
        let dir = tempfile::tempdir().unwrap();
        let gov = Governance::new(&wrap_opts(dir.path())).unwrap();
        let tool_v1 = serde_json::json!({
            "name": "read_file", "description": "v1",
            "inputSchema": {"type": "object"}
        });
        let mut resp = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {"tools": [tool_v1]}
        });
        gov.process_tools_list_response(&mut resp);
        assert_eq!(resp["result"]["tools"].as_array().unwrap().len(), 1);

        // Rug pull: same name, new description.
        let tool_v2 = serde_json::json!({
            "name": "read_file", "description": "v2 EVIL",
            "inputSchema": {"type": "object"}
        });
        let mut resp2 = serde_json::json!({
            "jsonrpc": "2.0", "id": 2,
            "result": {"tools": [tool_v2]}
        });
        gov.process_tools_list_response(&mut resp2);
        assert_eq!(
            resp2["result"]["tools"].as_array().unwrap().len(),
            0,
            "drifted tool must be quarantined out of the list"
        );
        assert!(gov.quarantined.lock().contains("read_file"));
    }
}

#[cfg(test)]
mod method_gate_tests {
    use super::*;

    #[test]
    fn method_allowlist_covers_core_mcp() {
        assert!(method_is_allowlisted("initialize"));
        assert!(method_is_allowlisted("tools/call"));
        assert!(method_is_allowlisted("resources/read"));
        assert!(method_is_allowlisted("notifications/progress"));
        assert!(!method_is_allowlisted("sampling/createMessage"));
    }
}
