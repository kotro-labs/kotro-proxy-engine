//! `kotro protect` / `kotro unprotect` — consent-driven rewriting of client
//! MCP configurations to route servers through `kotro-proxy mcp-wrap`.
//!
//! The original file is backed up to `<file>.kotro-backup` before the first
//! modification; `unprotect` restores that backup byte-for-byte.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub const BACKUP_SUFFIX: &str = ".kotro-backup";

/// Default config candidates for `kotro protect` without `--config`.
pub fn default_config_candidates(workspace: &Path) -> Vec<PathBuf> {
    let mut out = vec![
        workspace.join(".cursor/mcp.json"),
        workspace.join(".mcp.json"),
        workspace.join(".vscode/mcp.json"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        out.push(Path::new(&home).join(".cursor/mcp.json"));
    }
    out
}

pub struct ProtectOutcome {
    pub wrapped: Vec<String>,
    pub skipped: Vec<String>,
    pub backup_path: PathBuf,
}

/// Rewrite every `mcpServers` entry in `config_path` to run through
/// `wrapper_exe mcp-wrap`. Idempotent: already-wrapped servers are skipped.
pub fn protect(config_path: &Path, wrapper_exe: &Path) -> Result<ProtectOutcome, String> {
    let raw = std::fs::read_to_string(config_path)
        .map_err(|e| format!("read {}: {e}", config_path.display()))?;
    let mut root: Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", config_path.display()))?;

    let mut wrapped = Vec::new();
    let mut skipped = Vec::new();

    {
        let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
            return Err(format!("{}: no mcpServers object found", config_path.display()));
        };

        for (name, cfg) in servers.iter_mut() {
            if is_wrapped(cfg) {
                skipped.push(name.clone());
                continue;
            }
            let command = cfg.get("command").and_then(Value::as_str).map(str::to_string);
            let url = cfg.get("url").and_then(Value::as_str).map(str::to_string);
            let args: Vec<String> = cfg
                .get("args")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();

            let mut new_args: Vec<String> = vec!["mcp-wrap".into(), "--name".into(), name.clone()];
            match (&command, &url) {
                (Some(cmd), _) if !cmd.is_empty() => {
                    new_args.push("--".into());
                    new_args.push(cmd.clone());
                    new_args.extend(args);
                }
                (_, Some(u)) if !u.is_empty() => {
                    new_args.push("--url".into());
                    new_args.push(u.clone());
                }
                _ => {
                    skipped.push(name.clone());
                    continue;
                }
            }

            let obj = cfg.as_object_mut().unwrap();
            obj.insert(
                "command".into(),
                Value::String(wrapper_exe.display().to_string()),
            );
            obj.insert(
                "args".into(),
                Value::Array(new_args.into_iter().map(Value::String).collect()),
            );
            // A remote entry becomes a stdio entry pointing at the wrapper.
            obj.remove("url");
            obj.remove("type");
            wrapped.push(name.clone());
        }
    }

    let backup_path = backup_path_for(config_path);
    if !wrapped.is_empty() {
        // Preserve the first pre-Kotro state; never clobber an older backup.
        if !backup_path.exists() {
            std::fs::write(&backup_path, &raw)
                .map_err(|e| format!("write backup {}: {e}", backup_path.display()))?;
        }
        let pretty = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
        std::fs::write(config_path, pretty)
            .map_err(|e| format!("write {}: {e}", config_path.display()))?;
    }

    Ok(ProtectOutcome {
        wrapped,
        skipped,
        backup_path,
    })
}

/// Restore the exact pre-protect config from the backup file.
pub fn unprotect(config_path: &Path) -> Result<(), String> {
    let backup_path = backup_path_for(config_path);
    if !backup_path.exists() {
        return Err(format!(
            "no backup found at {} — nothing to restore",
            backup_path.display()
        ));
    }
    let original = std::fs::read(&backup_path)
        .map_err(|e| format!("read backup {}: {e}", backup_path.display()))?;
    std::fs::write(config_path, original)
        .map_err(|e| format!("restore {}: {e}", config_path.display()))?;
    std::fs::remove_file(&backup_path)
        .map_err(|e| format!("remove backup {}: {e}", backup_path.display()))?;
    Ok(())
}

fn backup_path_for(config_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}{BACKUP_SUFFIX}", config_path.display()))
}

fn is_wrapped(cfg: &Value) -> bool {
    let cmd = cfg.get("command").and_then(Value::as_str).unwrap_or("");
    let has_wrap_arg = cfg
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().any(|v| v.as_str() == Some("mcp-wrap")))
        .unwrap_or(false);
    cmd.contains("kotro") && has_wrap_arg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(dir: &Path) -> PathBuf {
        let path = dir.join("mcp.json");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "files": {
                        "command": "npx",
                        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
                        "env": {"LOG": "1"}
                    },
                    "remote": {"url": "https://tools.example.com/mcp"}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        path
    }

    #[test]
    fn protect_wraps_stdio_and_remote() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(dir.path());
        let outcome = protect(&config, Path::new("/usr/local/bin/kotro-proxy")).unwrap();
        assert_eq!(outcome.wrapped.len(), 2);

        let root: Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        let files = &root["mcpServers"]["files"];
        assert_eq!(files["command"], "/usr/local/bin/kotro-proxy");
        let args: Vec<&str> = files["args"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(&args[..4], &["mcp-wrap", "--name", "files", "--"]);
        assert_eq!(args[4], "npx");
        // env is preserved for the child.
        assert_eq!(files["env"]["LOG"], "1");

        let remote = &root["mcpServers"]["remote"];
        let rargs: Vec<&str> = remote["args"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(rargs.contains(&"--url"));
        assert!(remote.get("url").is_none());
    }

    #[test]
    fn protect_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(dir.path());
        protect(&config, Path::new("/usr/local/bin/kotro-proxy")).unwrap();
        let second = protect(&config, Path::new("/usr/local/bin/kotro-proxy")).unwrap();
        assert!(second.wrapped.is_empty());
        assert_eq!(second.skipped.len(), 2);
    }

    #[test]
    fn unprotect_restores_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(dir.path());
        let original = std::fs::read_to_string(&config).unwrap();

        protect(&config, Path::new("/usr/local/bin/kotro-proxy")).unwrap();
        assert_ne!(std::fs::read_to_string(&config).unwrap(), original);

        unprotect(&config).unwrap();
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert!(!backup_path_for(&config).exists());
    }

    #[test]
    fn backup_not_clobbered_by_second_protect() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(dir.path());
        let original = std::fs::read_to_string(&config).unwrap();
        protect(&config, Path::new("/usr/local/bin/kotro-proxy")).unwrap();

        // Add a new unwrapped server, protect again — backup must still hold
        // the *original* pre-Kotro state.
        let mut root: Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        root["mcpServers"]["extra"] = serde_json::json!({"command": "deno", "args": ["run", "s.ts"]});
        std::fs::write(&config, serde_json::to_string_pretty(&root).unwrap()).unwrap();
        protect(&config, Path::new("/usr/local/bin/kotro-proxy")).unwrap();

        let backup = std::fs::read_to_string(backup_path_for(&config)).unwrap();
        assert_eq!(backup, original);
    }
}
