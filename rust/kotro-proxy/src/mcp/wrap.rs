//! `kotro-proxy mcp-wrap` — the governed MCP relay runtime.
//!
//! stdio mode:   client ⇄ (this process) ⇄ child MCP server process
//! HTTP mode:    client ⇄ (this process, stdio) ⇄ remote Streamable HTTP server
//!
//! Interception points:
//! - all inbound methods: kill switch + allowlist (deny unknown methods).
//! - `tools/list` responses: pin metadata, quarantine drift (rug pulls).
//! - cacheable list/read results: honor server `ttlMs` / `cacheScope` (SEP-2549).
//! - `tools/call` requests: quarantine → schema validation →
//!   deny/ask/allow policy (with approval grants and session labels);
//!   propagate W3C Trace Context from `params._meta` into flight events (SEP-414).
//! - Streamable HTTP mode: emit `MCP-Protocol-Version`, `Mcp-Method`, and
//!   `Mcp-Name` on upstream POSTs (SEP-2243 client side). Server-side
//!   header↔body agreement checks apply only if Kotro terminates HTTP as a
//!   server (out of scope for wrap-as-client).
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
    /// Compiled admitted schemas (absent when admission failed in audit mode).
    admitted: Mutex<HashMap<String, kotro_schema::AdmittedSchema>>,
    /// When true, schema quarantine / invalid args are enforced.
    enforces: bool,
    /// When false (`KOTRO_MODE=disabled`), skip policy/schema evaluation.
    evaluates: bool,
    /// SEP-2549 list/resource-read result cache.
    list_cache: super::list_cache::ListResultCache,
    /// Process/env identity (task, principal, agent) for flight + approvals.
    identity: crate::identity_ctx::IdentityContext,
    /// Optional verified TaskEnvelope gate (C6).
    task_gate: super::task_gate::TaskGate,
}

impl Governance {
    fn new(opts: &WrapOptions) -> Result<Self, String> {
        let engine = policy::load_policy(Some(&opts.workspace), Some(&opts.state_dir))?;
        let mode = {
            let raw = std::env::var("KOTRO_MODE")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| {
                    std::env::var("KOTRO_ENFORCEMENT_MODE")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_else(|| "enforce".into());
            kotro_types::EnforcementMode::parse(&raw)
        };
        let enforces = mode.enforces();
        let evaluates = mode.evaluates();
        let task_gate = super::task_gate::TaskGate::from_env()?;
        // Mirror schema-pool telemetry into process metrics when the proxy
        // control plane is co-located; atomics always update either way.
        let metrics = crate::metrics::MetricsRegistry::new();
        metrics.attach_schema_telemetry();
        std::mem::forget(metrics); // keep Prometheus counter Arcs alive for the process
        let mut identity = crate::identity_ctx::IdentityContext::from_env();
        let overlay = task_gate.identity_overlay();
        // Envelope identity wins over bare env when present.
        if task_gate.is_active() {
            if !overlay.task_id.is_empty() {
                identity.task_id = overlay.task_id;
            }
            if !overlay.parent_task_id.is_empty() {
                identity.parent_task_id = overlay.parent_task_id;
            }
            if !overlay.principal_subject.is_empty() {
                identity.principal_subject = overlay.principal_subject;
            }
            if !overlay.principal_issuer.is_empty() {
                identity.principal_issuer = overlay.principal_issuer;
            }
            if !overlay.agent_name.is_empty() {
                identity.agent_name = overlay.agent_name;
            }
        }
        Ok(Self {
            server: opts.name.clone(),
            engine,
            state_dir: opts.state_dir.clone(),
            reporter: Reporter::new(&opts.state_dir, opts.session.clone()),
            halted: Arc::new(AtomicBool::new(false)),
            quarantined: Mutex::new(HashSet::new()),
            schemas: Mutex::new(HashMap::new()),
            admitted: Mutex::new(HashMap::new()),
            enforces,
            evaluates,
            list_cache: super::list_cache::ListResultCache::new(),
            identity,
            task_gate,
        })
    }

    fn event(&self, kind: &str, tool: &str, detail: String, enforced: bool) {
        self.event_decision(kind, tool, detail, enforced, "", "", "");
    }

    fn event_with(
        &self,
        kind: &str,
        tool: &str,
        detail: String,
        enforced: bool,
        provenance: &str,
    ) {
        self.event_decision(kind, tool, detail, enforced, provenance, "", "");
    }

    fn event_decision(
        &self,
        kind: &str,
        tool: &str,
        detail: String,
        enforced: bool,
        provenance: &str,
        rule_id: &str,
        reason_code: &str,
    ) {
        self.event_decision_traced(
            kind,
            tool,
            detail,
            enforced,
            provenance,
            rule_id,
            reason_code,
            &super::trace::TraceContext::default(),
            &self.identity,
        );
    }

    fn event_decision_traced(
        &self,
        kind: &str,
        tool: &str,
        detail: String,
        enforced: bool,
        provenance: &str,
        rule_id: &str,
        reason_code: &str,
        trace: &super::trace::TraceContext,
        identity: &crate::identity_ctx::IdentityContext,
    ) {
        let policy_revision = self.engine.revision();
        let decision_id = {
            let material = serde_json::json!({
                "session": self.reporter.session,
                "server": self.server,
                "tool": tool,
                "kind": kind,
                "rule_id": rule_id,
                "reason_code": reason_code,
                "policy_revision": policy_revision,
                "detail": detail,
            });
            let bytes = serde_json::to_vec(&material).unwrap_or_default();
            kotro_types::DecisionId::from_fingerprint(&bytes).0
        };
        let mut draft = serde_json::json!({
            "plane": "mcp",
            "kind": kind,
            "server": self.server,
            "tool_name": tool,
            "route": "mcp-wrap",
            "detail": detail,
            "enforced": enforced,
            "provenance": provenance,
            "decision_id": decision_id,
            "rule_id": rule_id,
            "policy_revision": policy_revision,
            "trace_id": trace.trace_id,
            "span_id": trace.span_id,
        });
        if let Some(obj) = draft.as_object_mut() {
            obj.extend(identity.to_report_fields());
        }
        self.reporter.report(draft);
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
            rule_id,
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
        let trace = super::trace::TraceContext::from_rpc_params(&params);
        let mut call_identity = self.identity.clone();
        if let Some(meta) = params.get("_meta") {
            call_identity.merge_mcp_meta(meta);
        }
        // Bound encoded argument size before any schema work (MCP Value path).
        if serde_json::to_vec(&args).map(|b| b.len()).unwrap_or(usize::MAX)
            > kotro_schema::ResourceLimits::HARD.encoded_arguments_size
        {
            self.event(
                "tool_denied",
                &tool,
                "schema:arguments_oversized".into(),
                self.enforces,
            );
            if self.enforces {
                return Err((ERR_INVALID_ARGS, "[KOTRO SCHEMA] arguments oversized".into()));
            }
        }

        // 1. Multi-plane kill switch (honored even when mode=disabled).
        if self.halted.load(Ordering::Relaxed) {
            self.event("tool_denied", &tool, "kill switch engaged (tools halted)".into(), true);
            return Err((
                ERR_KILL_SWITCH,
                "[KOTRO KILL SWITCH] tool execution halted by operator".into(),
            ));
        }

        // Disabled mode: skip policy / schema / task evaluation.
        if !self.evaluates {
            return Ok(());
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

        // 3. Argument validation against the admitted/pinned schema.
        {
            let admitted = self.admitted.lock().get(&tool).cloned();
            if let Some(schema) = admitted {
                let result = schema.validate_value(&args);
                if !result.ok {
                    let reason = kotro_schema::apply_mode(self.enforces, result.reason);
                    let detail = result
                        .errors
                        .iter()
                        .map(|e| e.display())
                        .collect::<Vec<_>>()
                        .join("; ");
                    let detail = if detail.is_empty() {
                        result.detail.clone()
                    } else {
                        detail
                    };
                    self.event(
                        "tool_denied",
                        &tool,
                        format!("schema:{reason:?}: {detail}"),
                        self.enforces,
                    );
                    if self.enforces {
                        return Err((
                            ERR_INVALID_ARGS,
                            format!("[KOTRO SCHEMA] arguments rejected: {detail}"),
                        ));
                    }
                }
            } else if self.schemas.lock().contains_key(&tool) {
                // Schema present but not admitted — enforce blocks; audit continues.
                self.event(
                    "tool_denied",
                    &tool,
                    "schema:validation_unavailable (not admitted)".into(),
                    self.enforces,
                );
                if self.enforces {
                    return Err((
                        ERR_INVALID_ARGS,
                        "[KOTRO SCHEMA] tool schema not admitted".into(),
                    ));
                }
            }
        }

        // 4. TaskEnvelope capability + budget gate (C6).
        {
            let args_hash = kotro_schema::args_hash(&args).unwrap_or_default();
            let schema_digest = self
                .admitted
                .lock()
                .get(&tool)
                .map(|s| s.digest.clone())
                .unwrap_or_default();
            if let Err(reason) =
                self.task_gate
                    .check_tool_call(&self.server, &tool, &args_hash, &schema_digest)
            {
                let detail = format!("task_envelope:{reason}");
                self.event_decision_traced(
                    "tool_denied",
                    &tool,
                    detail.clone(),
                    self.enforces,
                    "",
                    "task-envelope",
                    reason.as_str(),
                    &trace,
                    &call_identity,
                );
                if self.enforces {
                    return Err((
                        ERR_POLICY_DENIED,
                        format!("[KOTRO TASK] {reason}"),
                    ));
                }
            }
        }

        // 5. Policy evaluation with session provenance labels.
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
                self.event_decision_traced(
                    "tool_call",
                    &tool,
                    format!("allowed by {} ({})", decision.rule_id, decision.evidence),
                    false,
                    &provenance,
                    &decision.rule_id,
                    "allow",
                    &trace,
                    &call_identity,
                );
                Ok(())
            }
            policy::Action::Deny => {
                self.event_decision_traced(
                    "tool_denied",
                    &tool,
                    format!("denied by {} ({})", decision.rule_id, decision.evidence),
                    true,
                    &provenance,
                    &decision.rule_id,
                    "deny",
                    &trace,
                    &call_identity,
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
                let args_hash = kotro_schema::args_hash(&args).unwrap_or_else(|_| {
                    format!("sha256:{}", short_sha256(args.to_string().as_bytes()))
                });
                let schema_digest = self
                    .admitted
                    .lock()
                    .get(&tool)
                    .map(|s| s.digest.clone())
                    .unwrap_or_default();
                if self
                    .reporter
                    .check_approval(
                        &self.server,
                        &tool,
                        &args_hash,
                        &call_identity.task_id,
                        &schema_digest,
                        &decision.evidence,
                    )
                    .await
                {
                    self.event_decision_traced(
                        "tool_call",
                        &tool,
                        format!("approved grant matched ({})", decision.rule_id),
                        false,
                        &provenance,
                        &decision.rule_id,
                        "approved",
                        &trace,
                    &call_identity,
                    );
                    return Ok(());
                }
                self.event_decision_traced(
                    "tool_denied",
                    &tool,
                    format!(
                        "requires approval ({}, {}) — no grant found",
                        decision.rule_id, decision.evidence
                    ),
                    true,
                    &provenance,
                    &decision.rule_id,
                    "ask",
                    &trace,
                    &call_identity,
                );
                Err((
                    ERR_POLICY_DENIED,
                    format!(
                        "[KOTRO APPROVAL REQUIRED] '{tool}' needs approval ({}). Grant it with: \
                         kotro-proxy approve --server {} --tool {tool} --args-hash {args_hash}{}",
                        decision.evidence,
                        self.server,
                        if call_identity.task_id.is_empty() {
                            String::new()
                        } else {
                            format!(" --task-id {}", call_identity.task_id)
                        }
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
        let limits = kotro_schema::ResourceLimits::initial();
        let mut kept = Vec::new();
        {
            let mut schemas = self.schemas.lock();
            let mut admitted = self.admitted.lock();
            let mut quarantined = self.quarantined.lock();
            // Drop prior admissions for tools in this list so a failed recompile
            // cannot leave a stale validator active.
            for tool in &outcome.filtered_tools {
                if let Some(name) = tool.get("name").and_then(Value::as_str) {
                    admitted.remove(name);
                    schemas.remove(name);
                }
            }
            for tool in &outcome.filtered_tools {
                let Some(name) = tool.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let Some(schema) = tool.get("inputSchema") else {
                    self.event(
                        "tool_drift",
                        name,
                        "schema would_quarantine: missing inputSchema".into(),
                        self.enforces,
                    );
                    if self.enforces {
                        quarantined.insert(name.to_string());
                    } else {
                        kept.push(tool.clone());
                    }
                    continue;
                };
                schemas.insert(name.to_string(), schema.clone());
                match kotro_schema::compile(schema, &limits) {
                    Ok(compiled) => {
                        admitted.insert(name.to_string(), compiled);
                        kept.push(tool.clone());
                    }
                    Err(err) => {
                        let detail = err.to_string();
                        // Ensure no stale admission remains for this tool.
                        admitted.remove(name);
                        self.event(
                            "tool_drift",
                            name,
                            format!("schema would_quarantine: {detail}"),
                            self.enforces,
                        );
                        if self.enforces {
                            quarantined.insert(name.to_string());
                        } else {
                            kept.push(tool.clone());
                        }
                    }
                }
            }
        }

        if let Some(result) = response.get_mut("result") {
            result["tools"] = Value::Array(kept);
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
    // `initialize` kept for back-compat with pre-2026-07-28 clients; modern
    // clients use `server/discover`. `logging/setLevel` is deprecated but still
    // relayed. `tasks/*` is required when the Tasks extension is negotiated.
    matches!(
        method,
        "initialize"
            | "server/discover"
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
            | "logging/setLevel" // deprecated in 2026-07-28; still relayed
    ) || method.starts_with("notifications/")
        || method.starts_with("tasks/")
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
                "[KOTRO] MCP method '{method}' is not allowlisted by mcp-wrap; \
                 only initialize/server.discover/ping/tools/resources/prompts/\
                 completion/logging/tasks and notifications are relayed"
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
    // Cacheable method ids → (method, params) for SEP-2549 result caching.
    let pending_cacheable: Arc<Mutex<HashMap<String, (String, Value)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    // Forwarded tools/call ids → tool name, for output secret scanning.
    let pending_calls: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let _poller = spawn_kill_switch_poller(gov.clone(), child_pid);

    // Child stdout → our stdout (intercept tools/list results).
    let out_task = {
        let gov = gov.clone();
        let stdout = stdout.clone();
        let pending_cacheable = pending_cacheable.clone();
        let pending_calls = pending_calls.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(child_stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let emitted = if let Some(msg) = parse_message(&line) {
                    // Server-push list_changed → invalidate SEP-2549 cache.
                    if let Some(method) = msg.method.as_deref() {
                        if let Some(target) = super::list_cache::list_changed_target(method) {
                            gov.list_cache.invalidate_method(target);
                        }
                    }
                    if let Some(id) = &msg.id {
                        if let Some(tool) = pending_calls.lock().remove(&id_key(id)) {
                            gov.scan_tool_output(&tool, &line);
                        }
                        if let Some((method, params)) =
                            pending_cacheable.lock().remove(&id_key(id))
                        {
                            gov.list_cache.put(
                                &gov.reporter.session,
                                &method,
                                &params,
                                &msg.raw,
                            );
                            let mut modified = msg.raw.clone();
                            if method == "tools/list" {
                                gov.process_tools_list_response(&mut modified);
                            }
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
                    } else if super::list_cache::is_cacheable_method(method) {
                        let params = msg
                            .raw
                            .get("params")
                            .cloned()
                            .unwrap_or(Value::Null);
                        if let Some(cached) =
                            gov.list_cache
                                .get(&gov.reporter.session, method, &params)
                        {
                            forward = false;
                            let mut modified = cached;
                            // Preserve the client's request id on the cached body.
                            if let Some(id) = &msg.id {
                                modified["id"] = id.clone();
                            }
                            if method == "tools/list" {
                                gov.process_tools_list_response(&mut modified);
                            }
                            let out_line = modified.to_string();
                            let mut out = stdout.lock().await;
                            let _ = out.write_all(out_line.as_bytes()).await;
                            let _ = out.write_all(b"\n").await;
                            let _ = out.flush().await;
                        } else if let Some(id) = &msg.id {
                            pending_cacheable
                                .lock()
                                .insert(id_key(id), (method.to_string(), params));
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
        let params = msg.raw.get("params").cloned().unwrap_or(Value::Null);
        let is_tools_list = method == "tools/list";
        let is_tools_call = method == "tools/call";
        let call_tool = params
            .get("name")
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

        // SEP-2549: serve fresh cached list/read results without an upstream round-trip.
        if super::list_cache::is_cacheable_method(method) {
            if let Some(mut cached) =
                gov.list_cache
                    .get(&gov.reporter.session, method, &params)
            {
                if let Some(id) = &msg.id {
                    cached["id"] = id.clone();
                }
                if is_tools_list {
                    gov.process_tools_list_response(&mut cached);
                }
                write_line(&stdout, &cached.to_string()).await;
                continue;
            }
        }

        let routing = super::routing::RoutingHeaders::from_rpc(method, &params);
        // Defense in depth: never emit headers that disagree with the body.
        if let Err(disagree) = super::routing::validate_agreement(
            Some(routing.method.as_str()),
            routing.name.as_deref(),
            method,
            &params,
        ) {
            if let Some(id) = &msg.id {
                write_line(
                    &stdout,
                    &rpc_error(
                        id,
                        ERR_POLICY_DENIED,
                        &format!("[KOTRO] routing header agreement: {disagree}"),
                    ),
                )
                .await;
            }
            continue;
        }
        let mut req = client
            .post(&url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            // SEP-2243 routing headers — wrap acts as Streamable HTTP *client*.
            .header("MCP-Protocol-Version", routing.protocol_version)
            .header("Mcp-Method", routing.method.as_str())
            .body(line.clone());
        if let Some(name) = &routing.name {
            req = req.header("Mcp-Name", name.as_str());
        }
        if let Some(sid) = &mcp_session {
            // Legacy servers may still expect a session id; modern 2026-07-28
            // servers ignore it. Kept for back-compat with older remotes.
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
                emit_response(&gov, &stdout, data, method, &params).await;
            }
        } else {
            emit_response(&gov, &stdout, &body, method, &params).await;
        }
    }
    Ok(())
}

async fn emit_response(
    gov: &Arc<Governance>,
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    payload: &str,
    method: &str,
    params: &Value,
) {
    if let Ok(mut v) = serde_json::from_str::<Value>(payload) {
        if v.get("result").is_some() && super::list_cache::is_cacheable_method(method) {
            gov.list_cache
                .put(&gov.reporter.session, method, params, &v);
        }
        if method == "tools/list"
            && v.get("result")
                .map(|r| r.get("tools").is_some())
                .unwrap_or(false)
        {
            gov.process_tools_list_response(&mut v);
            write_line(stdout, &v.to_string()).await;
            return;
        }
        if let Some(note) = v.get("method").and_then(Value::as_str) {
            if let Some(target) = super::list_cache::list_changed_target(note) {
                gov.list_cache.invalidate_method(target);
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
        assert!(method_is_allowlisted("initialize")); // back-compat
        assert!(method_is_allowlisted("server/discover"));
        assert!(method_is_allowlisted("tools/call"));
        assert!(method_is_allowlisted("resources/read"));
        assert!(method_is_allowlisted("notifications/progress"));
        assert!(method_is_allowlisted("tasks/get"));
        assert!(method_is_allowlisted("tasks/list"));
        assert!(!method_is_allowlisted("sampling/createMessage"));
    }
}
