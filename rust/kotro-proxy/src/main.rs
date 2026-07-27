//! `kotro-proxy` — single-binary local LLM reverse proxy (Rust Phase 2).

use kotro_proxy::{config::Config, server::Server};
use std::env;
use tracing::info;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `kotro-proxy doctor [--json] [--pin] [--workspace <path>]`
fn run_doctor(args: &[String]) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let pin = args.iter().any(|a| a == "--pin");
    let workspace = args
        .iter()
        .position(|a| a == "--workspace")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    let state_dir = std::path::PathBuf::from(
        env::var("KOTRO_STATE_DIR").unwrap_or_else(|_| kotro_proxy::config::default_state_dir()),
    );

    let report = kotro_proxy::posture::run_doctor(&workspace, Some(&state_dir));

    if pin {
        match kotro_proxy::posture::pins::pin_servers(&state_dir, &report.servers) {
            Ok(n) => eprintln!("pinned {n} server baseline(s) → {}", state_dir.join("pins.json").display()),
            Err(e) => {
                eprintln!("failed to write pins: {e}");
                return 1;
            }
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
        );
    } else {
        print!("{}", kotro_proxy::posture::render_text(&report));
    }

    let (_, _, critical) = report.counts();
    if critical > 0 {
        2
    } else {
        0
    }
}

fn state_dir_path() -> std::path::PathBuf {
    std::path::PathBuf::from(
        env::var("KOTRO_STATE_DIR").unwrap_or_else(|_| kotro_proxy::config::default_state_dir()),
    )
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

/// `kotro-proxy mcp-wrap --name <server> [--url <url>] [--session <id>] [-- cmd args…]`
async fn run_mcp_wrap(args: &[String]) -> i32 {
    let name = match arg_value(args, "--name") {
        Some(n) => n,
        None => {
            eprintln!("mcp-wrap: --name <server> is required");
            return 1;
        }
    };
    let url = arg_value(args, "--url");
    let session = arg_value(args, "--session").unwrap_or_else(|| {
        format!("mcp-{}-{}", name, std::process::id())
    });
    let command: Vec<String> = args
        .iter()
        .position(|a| a == "--")
        .map(|i| args[i + 1..].to_vec())
        .unwrap_or_default();

    let opts = kotro_proxy::mcp::wrap::WrapOptions {
        name,
        command,
        url,
        state_dir: state_dir_path(),
        workspace: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        session,
    };
    match kotro_proxy::mcp::wrap::run_wrap(opts).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// `kotro-proxy protect [--config <path>]` / `unprotect [--config <path>]`
fn run_protect(args: &[String], undo: bool) -> i32 {
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let config = match arg_value(args, "--config").map(std::path::PathBuf::from) {
        Some(p) => p,
        None => {
            let candidates = kotro_proxy::mcp::protect::default_config_candidates(&workspace);
            match candidates.iter().find(|p| p.is_file()) {
                Some(p) => p.clone(),
                None => {
                    eprintln!(
                        "no MCP config found; pass --config <path> (looked for: {})",
                        candidates
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    return 1;
                }
            }
        }
    };

    if undo {
        match kotro_proxy::mcp::protect::unprotect(&config) {
            Ok(()) => {
                println!("restored {} from backup", config.display());
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }
    } else {
        let exe = std::env::current_exe().unwrap_or_else(|_| "kotro-proxy".into());
        match kotro_proxy::mcp::protect::protect(&config, &exe) {
            Ok(outcome) => {
                println!(
                    "wrapped {} server(s) in {} (backup: {})",
                    outcome.wrapped.len(),
                    config.display(),
                    outcome.backup_path.display()
                );
                for name in &outcome.wrapped {
                    println!("  protected: {name}");
                }
                for name in &outcome.skipped {
                    println!("  skipped (already protected / no launch info): {name}");
                }
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }
    }
}

/// `kotro-proxy approve --server <s> --tool <t> --args-hash <h> [--session <id>] [--ttl <secs>]`
///
/// Grants a short-lived approval for an ask-class tool call via the proxy's
/// authenticated control API. The exact command to run is printed in the
/// `[KOTRO APPROVAL REQUIRED]` error the blocked agent received.
async fn run_approve(args: &[String]) -> i32 {
    let (Some(server), Some(tool), Some(args_hash)) = (
        arg_value(args, "--server"),
        arg_value(args, "--tool"),
        arg_value(args, "--args-hash"),
    ) else {
        eprintln!("approve: --server, --tool, and --args-hash are required");
        return 1;
    };
    let session = arg_value(args, "--session");
    let ttl_secs = arg_value(args, "--ttl")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);

    let state_dir = state_dir_path();
    let token = env::var("KOTRO_CONTROL_TOKEN").ok().filter(|t| !t.trim().is_empty()).or_else(|| {
        std::fs::read_to_string(
            state_dir.join(kotro_proxy::router::control_auth::CONTROL_TOKEN_FILE),
        )
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    });
    let Some(token) = token else {
        eprintln!(
            "approve: no control token found (set KOTRO_CONTROL_TOKEN or start the proxy once \
             to create {})",
            state_dir.join("control.token").display()
        );
        return 1;
    };

    let metrics_addr = env::var("KOTRO_METRICS_ADDR").unwrap_or_else(|_| "127.0.0.1:9090".into());
    let host = if metrics_addr.starts_with(':') {
        format!("127.0.0.1{metrics_addr}")
    } else {
        metrics_addr
    };
    let body = serde_json::json!({
        "server": server,
        "tool": tool,
        "args_hash": args_hash,
        "session": session,
        "ttl_secs": ttl_secs,
    });
    let resp = reqwest::Client::new()
        .post(format!("http://{host}/api/approvals"))
        .header("x-kotro-control-token", token)
        .json(&body)
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            println!(
                "approved '{tool}' on '{server}' (args {args_hash}) for {ttl_secs}s{}",
                session.map(|s| format!(" in session {s}")).unwrap_or_default()
            );
            0
        }
        Ok(r) => {
            eprintln!("approve failed: proxy returned {}", r.status());
            1
        }
        Err(e) => {
            eprintln!("approve failed: proxy unreachable at {host}: {e}");
            1
        }
    }
}

/// `kotro-proxy hook claude-code` (stdin event) /
/// `kotro-proxy hook install|uninstall claude-code [--config <path>]`
async fn run_hook(args: &[String]) -> i32 {
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        // Enforcement handler: read one Claude Code hook event from stdin.
        (Some("claude-code"), _) => {
            use std::io::Read;
            let mut buf = String::new();
            // Fail closed: empty or unparseable stdin must deny PreToolUse.
            // A misconfigured or crashed hook must not silently allow tools.
            let deny_malformed = || {
                println!(
                    "{}",
                    serde_json::json!({
                        "hookSpecificOutput": {
                            "hookEventName": "PreToolUse",
                            "permissionDecision": "deny",
                            "permissionDecisionReason":
                                "[KOTRO] hook received empty or invalid event — fail closed",
                        }
                    })
                );
                0
            };
            if std::io::stdin().read_to_string(&mut buf).is_err() || buf.trim().is_empty() {
                return deny_malformed();
            }
            let raw: serde_json::Value = match serde_json::from_str(&buf) {
                Ok(v) => v,
                Err(_) => return deny_malformed(),
            };
            let event = kotro_proxy::hook::HookEvent::parse(&raw);
            let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let state_dir = state_dir_path();
            let outcome =
                kotro_proxy::hook::evaluate(&event, &workspace, &state_dir).await;
            let response = kotro_proxy::hook::render_response(&event, &outcome);
            println!("{response}");
            // Exit 0 always: the decision is carried in the JSON body so that
            // Claude Code surfaces our reason rather than a generic hook error.
            0
        }
        (Some("install"), Some("claude-code")) => {
            let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let config = arg_value(args, "--config").map(std::path::PathBuf::from);
            let exe = std::env::current_exe().unwrap_or_else(|_| "kotro-proxy".into());
            match kotro_proxy::hook::install_claude_code(&workspace, config.as_deref(), &exe) {
                Ok(out) => {
                    if out.changed {
                        println!("installed Kotro hooks in {}", out.settings_path.display());
                        if let Some(b) = out.backup_path {
                            println!("  backup: {}", b.display());
                        }
                        println!("  restart Claude Code for the hooks to take effect");
                    } else {
                        println!("Kotro hooks already present in {}", out.settings_path.display());
                    }
                    0
                }
                Err(e) => {
                    eprintln!("hook install failed: {e}");
                    1
                }
            }
        }
        (Some("uninstall"), Some("claude-code")) => {
            let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let config = arg_value(args, "--config").map(std::path::PathBuf::from);
            match kotro_proxy::hook::uninstall_claude_code(&workspace, config.as_deref()) {
                Ok(out) => {
                    if out.changed {
                        println!("removed Kotro hooks from {}", out.settings_path.display());
                    } else {
                        println!("no Kotro hooks found in {}", out.settings_path.display());
                    }
                    0
                }
                Err(e) => {
                    eprintln!("hook uninstall failed: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!(
                "usage:\n  kotro-proxy hook claude-code            (hook handler; reads event on stdin)\n  \
                 kotro-proxy hook install claude-code [--config <path>]\n  \
                 kotro-proxy hook uninstall claude-code [--config <path>]"
            );
            1
        }
    }
}

/// `kotro-proxy mcp repin --server <name>`
fn run_mcp_subcommand(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("repin") => {
            let Some(server) = arg_value(args, "--server") else {
                eprintln!("mcp repin: --server <name> is required");
                return 1;
            };
            match kotro_proxy::mcp::pin::repin_server(&state_dir_path(), &server) {
                Ok(true) => {
                    println!("cleared tool pins for '{server}' — next tools/list re-pins current metadata");
                    0
                }
                Ok(false) => {
                    println!("no tool pins recorded for '{server}' (nothing to clear)");
                    0
                }
                Err(e) => {
                    eprintln!("repin failed: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("usage: kotro-proxy mcp repin --server <name>");
            1
        }
    }
}

/// `kotro-proxy policy init [--preset observe|developer|locked-down] [--path <file>]`
fn run_policy(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("init") => {
            let preset_name = arg_value(args, "--preset").unwrap_or_else(|| "developer".into());
            let Some(preset) = kotro_proxy::policy::presets::by_name(&preset_name) else {
                eprintln!("unknown preset '{preset_name}' (observe | developer | locked-down)");
                return 1;
            };
            let path = arg_value(args, "--path")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(kotro_proxy::policy::POLICY_FILE));
            if path.exists() {
                eprintln!("{} already exists — refusing to overwrite", path.display());
                return 1;
            }
            let yaml = serde_yaml::to_string(&preset).unwrap_or_default();
            let header = format!(
                "# kotro-policy.yaml — deny-first agent action policy (preset: {preset_name})\n\
                 # Precedence: deny > ask > allow > per-class defaults. Docs: docs/security/THREAT-MODEL.md\n"
            );
            if let Err(e) = std::fs::write(&path, format!("{header}{yaml}")) {
                eprintln!("write {}: {e}", path.display());
                return 1;
            }
            println!("wrote {} (preset: {preset_name})", path.display());
            0
        }
        Some("show") => {
            let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let state_dir = state_dir_path();
            match kotro_proxy::policy::resolve_policy(Some(&workspace), Some(&state_dir)) {
                Ok((file, sources)) => {
                    if args.iter().any(|a| a == "--json") {
                        println!("{}", serde_json::to_string_pretty(&file).unwrap_or_default());
                    } else {
                        if sources.is_empty() {
                            eprintln!("# effective policy: built-in '{}' preset (no policy file found)", file.preset);
                        } else {
                            eprintln!("# effective policy (layered):");
                            for s in &sources {
                                eprintln!("#   {}", s.display());
                            }
                        }
                        println!("{}", serde_yaml::to_string(&file).unwrap_or_default());
                    }
                    0
                }
                Err(e) => {
                    eprintln!("policy show failed: {e}");
                    1
                }
            }
        }
        Some("check") => run_policy_check(args),
        _ => {
            eprintln!(
                "usage:\n  kotro-proxy policy init [--preset observe|developer|locked-down] [--path <file>]\n  \
                 kotro-proxy policy show [--json]\n  \
                 kotro-proxy policy check --tool <name> [--server s] [--class c] [--path p] [--url u] [--exec e] [--label l] [--json]"
            );
            1
        }
    }
}

/// `kotro-proxy policy check` — explain the decision for a hypothetical call.
fn run_policy_check(args: &[String]) -> i32 {
    let Some(tool) = arg_value(args, "--tool") else {
        eprintln!("policy check: --tool <name> is required");
        return 1;
    };
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let state_dir = state_dir_path();
    let engine = match kotro_proxy::policy::load_policy(Some(&workspace), Some(&state_dir)) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("policy check: {e}");
            return 1;
        }
    };

    // Class: explicit --class, else classify from the tool name.
    let class = arg_value(args, "--class")
        .and_then(|c| kotro_proxy::policy::ToolClass::parse(&c))
        .unwrap_or_else(|| kotro_proxy::policy::classify_tool(&tool, None));

    let mut ctx = kotro_proxy::policy::ToolCallContext {
        server: arg_value(args, "--server").unwrap_or_default(),
        tool: tool.clone(),
        class,
        server_digest: arg_value(args, "--server-digest").unwrap_or_default(),
        ..Default::default()
    };
    if let Some(p) = arg_value(args, "--path") {
        ctx.paths.push(p);
    }
    if let Some(u) = arg_value(args, "--url") {
        ctx.domains.push(
            u.strip_prefix("https://")
                .or_else(|| u.strip_prefix("http://"))
                .unwrap_or(&u)
                .split(['/', ':', '?'])
                .next()
                .unwrap_or(&u)
                .to_string(),
        );
    }
    if let Some(e) = arg_value(args, "--exec") {
        ctx.executables.push(e);
    }
    for label in args
        .iter()
        .enumerate()
        .filter(|(_, a)| a.as_str() == "--label")
        .filter_map(|(i, _)| args.get(i + 1))
    {
        ctx.data_labels.push(label.clone());
    }

    let decision = engine.evaluate(&ctx);
    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "tool": tool,
                "class": class.as_str(),
                "preset": engine.preset(),
                "action": decision.action.as_str(),
                "rule_id": decision.rule_id,
                "evidence": decision.evidence,
            }))
            .unwrap_or_default()
        );
    } else {
        println!("tool:     {tool}");
        println!("class:    {}", class.as_str());
        println!("policy:   {}", engine.preset());
        println!("decision: {}", decision.action.as_str().to_uppercase());
        println!("rule:     {}", decision.rule_id);
        println!("evidence: {}", decision.evidence);
    }
    // Exit code reflects the decision: 0 allow, 1 ask, 2 deny.
    match decision.action {
        kotro_proxy::policy::Action::Allow => 0,
        kotro_proxy::policy::Action::Ask => 1,
        kotro_proxy::policy::Action::Deny => 2,
    }
}

/// `kotro-proxy isolate docker --name <s> --image <img> [--mount path] [--rw-mount path]
///   [--egress host] [--cpus 0.5] [--memory 256m] [--secrets-env file] [--out dir]`
fn run_isolate(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("docker") => {
            let Some(name) = arg_value(args, "--name") else {
                eprintln!("isolate docker: --name <server> is required");
                return 1;
            };
            let Some(image) = arg_value(args, "--image") else {
                eprintln!("isolate docker: --image <image> is required");
                return 1;
            };
            let mut opts = kotro_proxy::isolate::IsolateOptions {
                name,
                image,
                ..Default::default()
            };
            if let Some(c) = arg_value(args, "--cpus") {
                opts.cpus = c;
            }
            if let Some(m) = arg_value(args, "--memory") {
                opts.memory = m;
            }
            if let Some(s) = arg_value(args, "--secrets-env") {
                opts.secrets_env_file = Some(std::path::PathBuf::from(s));
            }
            // Multi-value flags: collect every occurrence.
            let mut i = 0;
            while i < args.len() {
                match args[i].as_str() {
                    "--mount" => {
                        if let Some(p) = args.get(i + 1) {
                            opts.read_only_mounts.push(std::path::PathBuf::from(p));
                            i += 2;
                            continue;
                        }
                    }
                    "--rw-mount" => {
                        if let Some(p) = args.get(i + 1) {
                            opts.read_write_mounts.push(std::path::PathBuf::from(p));
                            i += 2;
                            continue;
                        }
                    }
                    "--egress" => {
                        if let Some(h) = args.get(i + 1) {
                            opts.egress_hosts.push(h.clone());
                            i += 2;
                            continue;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            let out_dir = arg_value(args, "--out")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("kotro-isolate"));
            match kotro_proxy::isolate::write_profile(&out_dir, &opts) {
                Ok((compose, profile)) => {
                    println!("wrote {}", compose.display());
                    println!("wrote {}", profile.display());
                    println!(
                        "review and apply via Docker MCP Gateway — Kotro does not execute containers"
                    );
                    0
                }
                Err(e) => {
                    eprintln!("isolate failed: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!(
                "usage: kotro-proxy isolate docker --name <s> --image <img> \
                 [--mount path]… [--rw-mount path]… [--egress host]… \
                 [--cpus 0.5] [--memory 256m] [--secrets-env file] [--out dir]"
            );
            1
        }
    }
}

/// `kotro-proxy corpus list|run [--json]`
fn run_corpus(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") => {
            let scenarios = kotro_proxy::corpus::corpus();
            if args.iter().any(|a| a == "--json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&scenarios).unwrap_or_else(|_| "[]".into())
                );
            } else {
                for s in &scenarios {
                    println!("{:<28} [{:<12}] {}", s.id, s.category, s.title);
                }
            }
            0
        }
        Some("run") => {
            let scenarios = kotro_proxy::corpus::corpus();
            let mut failed = 0u32;
            for s in &scenarios {
                match kotro_proxy::corpus::run_scenario(s) {
                    Ok(()) => println!("PASS  {}", s.id),
                    Err(e) => {
                        println!("FAIL  {}: {e}", s.id);
                        failed += 1;
                    }
                }
            }
            if failed == 0 {
                println!("all {} scenarios passed", scenarios.len());
                0
            } else {
                eprintln!("{failed} scenario(s) failed");
                1
            }
        }
        _ => {
            eprintln!("usage: kotro-proxy corpus list|run [--json]");
            1
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("kotro-proxy {VERSION}");
        return Ok(());
    }

    match args.get(1).map(String::as_str) {
        Some("doctor") => {
            std::process::exit(run_doctor(&args[2..]));
        }
        Some("mcp-wrap") => {
            std::process::exit(run_mcp_wrap(&args[2..]).await);
        }
        Some("protect") => {
            std::process::exit(run_protect(&args[2..], false));
        }
        Some("unprotect") => {
            std::process::exit(run_protect(&args[2..], true));
        }
        Some("approve") => {
            std::process::exit(run_approve(&args[2..]).await);
        }
        Some("hook") => {
            std::process::exit(run_hook(&args[2..]).await);
        }
        Some("isolate") => {
            std::process::exit(run_isolate(&args[2..]));
        }
        Some("corpus") => {
            std::process::exit(run_corpus(&args[2..]));
        }
        Some("mcp") => {
            std::process::exit(run_mcp_subcommand(&args[2..]));
        }
        Some("policy") => {
            std::process::exit(run_policy(&args[2..]));
        }
        _ => {}
    }

    let cfg = Config::load();

    // Initialise telemetry and retain the provider handle so we can flush
    // buffered spans before exit (only Some when KOTRO_OTEL_ENDPOINT is set).
    let otel_provider = match kotro_proxy::telemetry::otel::init_telemetry(cfg.otel_endpoint.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to initialize telemetry: {e}");
            None
        }
    };

    let bridge_enabled = cfg
        .bridge_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some();
    if bridge_enabled && cfg.upstream_api_key.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_none() {
        tracing::warn!(
            "KOTRO_BRIDGE_TOKEN is set without KOTRO_UPSTREAM_API_KEY — \
             upstream LLM calls will return 503 until the provider key is configured"
        );
    }

    info!(
        service = "kotro-proxy",
        listen = %cfg.listen_addr,
        metrics = %cfg.metrics_addr,
        upstream = %cfg.upstream_url,
        fallback_configured = cfg.fallback_url.is_some(),
        bridge_auth = bridge_enabled,
        profile = %env::var("KOTRO_PROFILE").unwrap_or_default(),
        cache_strategy = ?cfg.cache_key_strategy,
        cache_window = cfg.cache_window_size,
        redaction = cfg.enable_redaction,
        compression = cfg.enable_compression,
        "starting kotrolabs proxy"
    );

    let server = Server::new(cfg)?;
    server.run().await?;

    // Flush any buffered OTel spans before the process exits.
    if let Some(provider) = otel_provider {
        if let Err(e) = provider.shutdown() {
            eprintln!("OTel provider shutdown error: {e}");
        }
    }

    Ok(())
}
