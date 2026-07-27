//! `kotro-proxy hook install/uninstall claude-code` — consent-driven editing
//! of Claude Code's `settings.json` hooks, with an exact-restore backup.
//!
//! We register one `PreToolUse` and one `PostToolUse` matcher that shell out
//! to `kotro-proxy hook claude-code`. Existing unrelated hooks are preserved;
//! re-running install is idempotent.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const MARKER: &str = "hook claude-code";

/// Candidate settings files: project-local first, then user-global.
fn settings_candidates(workspace: &Path) -> Vec<PathBuf> {
    let mut out = vec![
        workspace.join(".claude").join("settings.json"),
        workspace.join(".claude").join("settings.local.json"),
    ];
    if let Some(home) = dirs_home() {
        out.push(home.join(".claude").join("settings.json"));
    }
    out
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Resolve which settings file to edit. Honors an explicit path; otherwise
/// picks the first existing candidate, else the user-global default (created).
fn resolve_target(workspace: &Path, explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    let candidates = settings_candidates(workspace);
    if let Some(existing) = candidates.iter().find(|p| p.is_file()) {
        return existing.clone();
    }
    candidates
        .into_iter()
        .last()
        .unwrap_or_else(|| workspace.join(".claude").join("settings.json"))
}

fn hook_command(exe: &Path) -> String {
    format!("{} hook claude-code", exe.display())
}

fn kotro_matcher(exe: &Path) -> Value {
    json!({
        "matcher": "*",
        "hooks": [ { "type": "command", "command": hook_command(exe) } ]
    })
}

fn is_kotro_matcher(m: &Value) -> bool {
    m.get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| c.contains(MARKER))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Insert the Kotro matcher into `settings.hooks[event]`, skipping if already
/// present. Returns true when a change was made.
fn ensure_event(root: &mut Value, event: &str, exe: &Path) -> bool {
    let hooks = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let arr = hooks
        .as_object_mut()
        .unwrap()
        .entry(event)
        .or_insert_with(|| json!([]));
    let Some(list) = arr.as_array_mut() else {
        return false;
    };
    if list.iter().any(is_kotro_matcher) {
        return false;
    }
    list.push(kotro_matcher(exe));
    true
}

fn remove_event(root: &mut Value, event: &str) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(list) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
        return false;
    };
    let before = list.len();
    list.retain(|m| !is_kotro_matcher(m));
    before != list.len()
}

fn load_settings(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn write_settings(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, body + "\n").map_err(|e| format!("write {}: {e}", path.display()))
}

pub struct InstallOutcome {
    pub settings_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub changed: bool,
}

pub fn install_claude_code(
    workspace: &Path,
    explicit: Option<&Path>,
    exe: &Path,
) -> Result<InstallOutcome, String> {
    let path = resolve_target(workspace, explicit);
    let mut root = load_settings(&path)?;
    if !root.is_object() {
        return Err(format!("{}: expected a JSON object", path.display()));
    }

    // Back up an existing file exactly once before mutating.
    let backup_path = if path.is_file() {
        let backup = path.with_extension("json.kotro-bak");
        if !backup.exists() {
            std::fs::copy(&path, &backup)
                .map_err(|e| format!("backup {}: {e}", backup.display()))?;
        }
        Some(backup)
    } else {
        None
    };

    let mut changed = ensure_event(&mut root, "PreToolUse", exe);
    changed |= ensure_event(&mut root, "PostToolUse", exe);
    if changed {
        write_settings(&path, &root)?;
    }
    Ok(InstallOutcome { settings_path: path, backup_path, changed })
}

pub fn uninstall_claude_code(
    workspace: &Path,
    explicit: Option<&Path>,
) -> Result<InstallOutcome, String> {
    let path = resolve_target(workspace, explicit);
    if !path.is_file() {
        return Ok(InstallOutcome { settings_path: path, backup_path: None, changed: false });
    }
    let mut root = load_settings(&path)?;
    let mut changed = remove_event(&mut root, "PreToolUse");
    changed |= remove_event(&mut root, "PostToolUse");
    // Drop now-empty hook event arrays / the hooks object.
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        hooks.retain(|_, v| v.as_array().map(|a| !a.is_empty()).unwrap_or(true));
        if hooks.is_empty() {
            root.as_object_mut().unwrap().remove("hooks");
        }
    }
    if changed {
        write_settings(&path, &root)?;
    }
    Ok(InstallOutcome { settings_path: path, backup_path: None, changed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent_and_preserves_existing_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let settings = ws.join(".claude").join("settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"other-tool"}]}]}}"#,
        )
        .unwrap();
        let exe = PathBuf::from("/usr/local/bin/kotro-proxy");

        let out = install_claude_code(ws, Some(&settings), &exe).unwrap();
        assert!(out.changed);
        assert!(out.backup_path.as_ref().unwrap().is_file());

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        // Existing hook preserved + kotro added.
        assert_eq!(pre.len(), 2);
        assert!(pre.iter().any(is_kotro_matcher));
        assert!(v["hooks"]["PostToolUse"].as_array().unwrap().iter().any(is_kotro_matcher));

        // Second install makes no further change.
        let out2 = install_claude_code(ws, Some(&settings), &exe).unwrap();
        assert!(!out2.changed);
    }

    #[test]
    fn uninstall_removes_only_kotro_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let settings = ws.join("settings.json");
        let exe = PathBuf::from("/usr/local/bin/kotro-proxy");
        std::fs::write(&settings, r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"other-tool"}]}]}}"#).unwrap();

        install_claude_code(ws, Some(&settings), &exe).unwrap();
        uninstall_claude_code(ws, Some(&settings)).unwrap();

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert!(!pre.iter().any(is_kotro_matcher));
        // PostToolUse only had the kotro hook → event removed entirely.
        assert!(v["hooks"].get("PostToolUse").is_none());
    }

    #[test]
    fn install_creates_missing_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(".claude").join("settings.json");
        let exe = PathBuf::from("/usr/local/bin/kotro-proxy");
        let out = install_claude_code(dir.path(), Some(&target), &exe).unwrap();
        assert!(out.changed);
        assert!(out.backup_path.is_none());
        assert!(target.is_file());
    }
}
