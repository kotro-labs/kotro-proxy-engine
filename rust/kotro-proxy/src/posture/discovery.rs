//! Discovery of local agent / MCP configurations, plus per-server risk checks.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Finding, McpServerRecord, Severity};

fn home() -> Option<PathBuf> {
    std::env::var("HOME").ok().filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Candidate config files: `(client, path)`. Only existing files are parsed.
fn candidate_configs(workspace: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut out: Vec<(&'static str, PathBuf)> = vec![
        ("claude-code", workspace.join(".mcp.json")),
        ("cursor", workspace.join(".cursor/mcp.json")),
        ("vscode", workspace.join(".vscode/mcp.json")),
    ];
    if let Some(h) = home() {
        out.push(("claude-code", h.join(".claude.json")));
        out.push(("cursor", h.join(".cursor/mcp.json")));
        out.push(("continue", h.join(".continue/config.json")));
        // Cline stores MCP settings in the VS Code global storage dir.
        out.push((
            "cline",
            h.join("Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
        ));
        out.push((
            "cline",
            h.join(".config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
        ));
    }
    out
}

/// Discover all MCP servers configured for supported clients.
pub fn discover_mcp_servers(workspace: &Path) -> Vec<McpServerRecord> {
    let mut servers = Vec::new();
    for (client, path) in candidate_configs(workspace) {
        if !path.is_file() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else { continue };
        let Ok(root) = serde_json::from_str::<Value>(&raw) else { continue };
        // `mcpServers` may sit at the top level (.mcp.json, cursor) or nested
        // (Claude ~/.claude.json has top-level and per-project entries).
        collect_mcp_servers(&root, client, &path, &mut servers);
        if let Some(projects) = root.get("projects").and_then(Value::as_object) {
            for project in projects.values() {
                collect_mcp_servers(project, client, &path, &mut servers);
            }
        }
    }
    servers
}

fn collect_mcp_servers(
    node: &Value,
    client: &str,
    source: &Path,
    out: &mut Vec<McpServerRecord>,
) {
    let Some(map) = node.get("mcpServers").and_then(Value::as_object) else {
        return;
    };
    for (name, cfg) in map {
        // Skip duplicates from the same (client, name) pair.
        if out.iter().any(|s| s.client == client && &s.name == name) {
            continue;
        }
        out.push(parse_server(name, cfg, client, source));
    }
}

fn parse_server(name: &str, cfg: &Value, client: &str, source: &Path) -> McpServerRecord {
    let command = cfg.get("command").and_then(Value::as_str).unwrap_or("").to_string();
    let args: Vec<String> = cfg
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let url = cfg.get("url").and_then(Value::as_str).unwrap_or("").to_string();
    let explicit_type = cfg.get("type").and_then(Value::as_str).unwrap_or("");
    let transport = if !url.is_empty() {
        if explicit_type == "sse" { "sse" } else { "http" }
    } else {
        "stdio"
    }
    .to_string();
    let env_keys: Vec<String> = cfg
        .get("env")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();

    let executable_path = if command.is_empty() {
        String::new()
    } else {
        resolve_executable(&command).map(|p| p.display().to_string()).unwrap_or_default()
    };
    let executable_digest = if executable_path.is_empty() {
        String::new()
    } else {
        std::fs::read(&executable_path)
            .map(|bytes| super::short_sha256(&bytes))
            .unwrap_or_default()
    };

    // Normalized config digest = command + args + url + sorted env keys.
    let mut material = format!("{command}\x1f{}\x1f{url}", args.join("\x1f"));
    let mut sorted_env = env_keys.clone();
    sorted_env.sort();
    material.push('\x1f');
    material.push_str(&sorted_env.join(","));
    let config_digest = super::short_sha256(material.as_bytes());

    let protected = command.contains("kotro") && args.iter().any(|a| a == "mcp-wrap");

    McpServerRecord {
        name: name.to_string(),
        client: client.to_string(),
        source_path: source.display().to_string(),
        transport,
        command,
        args,
        url,
        env_keys,
        executable_path,
        executable_digest,
        config_digest,
        protected,
    }
}

fn resolve_executable(command: &str) -> Option<PathBuf> {
    let p = Path::new(command);
    if p.is_absolute() && p.is_file() {
        return Some(p.to_path_buf());
    }
    let path_var = std::env::var("PATH").ok()?;
    for dir in path_var.split(':') {
        let candidate = Path::new(dir).join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

const CREDENTIAL_PATH_HINTS: [&str; 7] = [
    ".ssh", "id_rsa", ".aws", ".env", "credentials", ".gnupg", ".netrc",
];

/// Deterministic risk checks for one MCP server entry.
pub fn assess_server(server: &McpServerRecord) -> Vec<Finding> {
    let mut findings = Vec::new();
    let subject = format!("{}:{}", server.client, server.name);

    if server.protected {
        return findings;
    }

    // Inline env vars: values are configured in the file itself. We only
    // record names, but names that look secret-bearing get flagged.
    for key in &server.env_keys {
        let upper = key.to_ascii_uppercase();
        if upper.contains("KEY") || upper.contains("TOKEN") || upper.contains("SECRET")
            || upper.contains("PASSWORD")
        {
            findings.push(Finding {
                severity: Severity::Warn,
                code: "MCP_INLINE_SECRET".into(),
                subject: subject.clone(),
                detail: format!(
                    "env var '{key}' is configured inline in {} — any process reading \
                     that file sees the credential",
                    server.source_path
                ),
            });
        }
    }

    // Remote transports.
    if !server.url.is_empty() {
        let loopback = server.url.contains("127.0.0.1") || server.url.contains("localhost");
        if server.url.starts_with("http://") && !loopback {
            findings.push(Finding {
                severity: Severity::Critical,
                code: "MCP_PLAINTEXT_URL".into(),
                subject: subject.clone(),
                detail: format!("remote MCP server over plaintext http: {}", server.url),
            });
        } else if !loopback {
            findings.push(Finding {
                severity: Severity::Info,
                code: "MCP_REMOTE_SERVER".into(),
                subject: subject.clone(),
                detail: format!(
                    "remote MCP server {} — tool descriptions enter model context from \
                     an external origin",
                    server.url
                ),
            });
        }
    }

    // Unpinned package-manager launches (supply chain).
    let cmd_base = server
        .command
        .rsplit('/')
        .next()
        .unwrap_or(&server.command)
        .to_string();
    if matches!(cmd_base.as_str(), "npx" | "uvx" | "bunx" | "pipx") {
        let pinned = server.args.iter().any(|a| a.contains('@') && !a.starts_with('@') || a.contains("=="));
        if !pinned {
            findings.push(Finding {
                severity: Severity::Warn,
                code: "MCP_UNPINNED_PACKAGE".into(),
                subject: subject.clone(),
                detail: format!(
                    "'{} {}' launches an unpinned package — a malicious release becomes \
                     code execution on next start (rug-pull surface)",
                    cmd_base,
                    server.args.join(" ")
                ),
            });
        }
    }

    // Shell-wrapped launches hide the real target.
    if matches!(cmd_base.as_str(), "sh" | "bash" | "zsh")
        && server.args.iter().any(|a| a == "-c")
    {
        findings.push(Finding {
            severity: Severity::Warn,
            code: "MCP_SHELL_WRAPPER".into(),
            subject: subject.clone(),
            detail: "server launched through 'sh -c' — actual executable is opaque to inventory".into(),
        });
    }

    // Broad filesystem access in args.
    for arg in &server.args {
        if arg == "/" || arg == "~" || arg == "$HOME" {
            findings.push(Finding {
                severity: Severity::Critical,
                code: "MCP_BROAD_FS".into(),
                subject: subject.clone(),
                detail: format!("server argument grants access to '{arg}' (entire filesystem/home)"),
            });
        }
        let lower = arg.to_ascii_lowercase();
        if CREDENTIAL_PATH_HINTS.iter().any(|hint| lower.contains(hint)) {
            findings.push(Finding {
                severity: Severity::Critical,
                code: "MCP_CREDENTIAL_PATH".into(),
                subject: subject.clone(),
                detail: format!("server argument references a credential path: '{arg}'"),
            });
        }
    }

    // Unresolvable executables can't be digested or pinned.
    if !server.command.is_empty() && server.executable_path.is_empty() {
        findings.push(Finding {
            severity: Severity::Info,
            code: "MCP_EXEC_UNRESOLVED".into(),
            subject,
            detail: format!(
                "command '{}' not found on PATH — executable digest unavailable",
                server.command
            ),
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_from_json(name: &str, cfg: serde_json::Value) -> McpServerRecord {
        parse_server(name, &cfg, "cursor", Path::new("/tmp/mcp.json"))
    }

    #[test]
    fn discovers_workspace_mcp_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cursor")).unwrap();
        std::fs::write(
            dir.path().join(".cursor/mcp.json"),
            r#"{"mcpServers":{"files":{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","/"]}}}"#,
        )
        .unwrap();
        let servers = discover_mcp_servers(dir.path());
        assert_eq!(servers.iter().filter(|s| s.name == "files").count(), 1);
        let s = servers.iter().find(|s| s.name == "files").unwrap();
        assert_eq!(s.transport, "stdio");
        assert_eq!(s.command, "npx");
    }

    #[test]
    fn flags_broad_fs_and_inline_secret() {
        let s = server_from_json(
            "danger",
            serde_json::json!({
                "command": "npx",
                "args": ["-y", "some-server", "/"],
                "env": {"GITHUB_TOKEN": "ghp_abc"}
            }),
        );
        let findings = assess_server(&s);
        let codes: Vec<&str> = findings.iter().map(|f| f.code.as_str()).collect();
        assert!(codes.contains(&"MCP_BROAD_FS"), "{codes:?}");
        assert!(codes.contains(&"MCP_INLINE_SECRET"), "{codes:?}");
        assert!(codes.contains(&"MCP_UNPINNED_PACKAGE"), "{codes:?}");
    }

    #[test]
    fn flags_plaintext_remote_url() {
        let s = server_from_json(
            "remote",
            serde_json::json!({"url": "http://tools.example.com/mcp"}),
        );
        let findings = assess_server(&s);
        assert!(findings.iter().any(|f| f.code == "MCP_PLAINTEXT_URL"
            && f.severity == Severity::Critical));
    }

    #[test]
    fn loopback_url_not_flagged_as_remote() {
        let s = server_from_json("local", serde_json::json!({"url": "http://127.0.0.1:8321/mcp"}));
        let findings = assess_server(&s);
        assert!(!findings.iter().any(|f| f.code == "MCP_PLAINTEXT_URL"));
        assert!(!findings.iter().any(|f| f.code == "MCP_REMOTE_SERVER"));
    }

    #[test]
    fn protected_servers_are_skipped() {
        let s = server_from_json(
            "wrapped",
            serde_json::json!({
                "command": "/usr/local/bin/kotro-proxy",
                "args": ["mcp-wrap", "--name", "files", "--", "npx", "-y", "server", "/"]
            }),
        );
        assert!(s.protected);
        assert!(assess_server(&s).is_empty());
    }

    #[test]
    fn credential_path_arg_is_critical() {
        let s = server_from_json(
            "creds",
            serde_json::json!({"command": "node", "args": ["server.js", "/Users/me/.ssh"]}),
        );
        let findings = assess_server(&s);
        assert!(findings.iter().any(|f| f.code == "MCP_CREDENTIAL_PATH"
            && f.severity == Severity::Critical));
    }
}
