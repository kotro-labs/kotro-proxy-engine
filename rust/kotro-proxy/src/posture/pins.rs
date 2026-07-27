//! Baseline pinning for approved MCP server configurations and tool metadata.
//!
//! `kotro doctor --pin` records the current config/executable digests. On
//! later runs, drift against the approved baseline is flagged (MCP rug pulls:
//! a server the user approved silently changing its config, binary, or —
//! via the action plane — its tool descriptions/schemas).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Finding, McpServerRecord, Severity};

pub const PINS_FILE: &str = "pins.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerPin {
    #[serde(default)]
    pub config_digest: String,
    #[serde(default)]
    pub executable_digest: String,
    /// Per-tool digest of name + description + input schema + annotations,
    /// recorded by the MCP action plane on first approved `tools/list`.
    #[serde(default)]
    pub tool_digests: BTreeMap<String, String>,
    #[serde(default)]
    pub pinned_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PinSet {
    /// Keyed by `<client>:<name>` for config pins and `<server>` for
    /// action-plane tool pins.
    #[serde(default)]
    pub servers: BTreeMap<String, ServerPin>,
}

pub fn load(state_dir: &Path) -> PinSet {
    let path = state_dir.join(PINS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => PinSet::default(),
    }
}

pub fn save(state_dir: &Path, pins: &PinSet) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let raw = serde_json::to_string_pretty(pins).unwrap_or_else(|_| "{}".into());
    std::fs::write(state_dir.join(PINS_FILE), raw)
}

/// Record the current server digests as the approved baseline.
pub fn pin_servers(state_dir: &Path, servers: &[McpServerRecord]) -> std::io::Result<usize> {
    let mut pins = load(state_dir);
    let mut count = 0;
    for s in servers {
        let key = format!("{}:{}", s.client, s.name);
        let entry = pins.servers.entry(key).or_default();
        entry.config_digest = s.config_digest.clone();
        entry.executable_digest = s.executable_digest.clone();
        entry.pinned_at = crate::flight_recorder::now_rfc3339();
        count += 1;
    }
    save(state_dir, &pins)?;
    Ok(count)
}

/// Compare discovered servers to the pinned baseline.
pub fn detect_drift(pins: &PinSet, servers: &[McpServerRecord]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for s in servers {
        let key = format!("{}:{}", s.client, s.name);
        let Some(pin) = pins.servers.get(&key) else { continue };
        if !pin.config_digest.is_empty() && pin.config_digest != s.config_digest {
            findings.push(Finding {
                severity: Severity::Critical,
                code: "MCP_CONFIG_DRIFT".into(),
                subject: key.clone(),
                detail: format!(
                    "server config changed since it was pinned ({} → {}) — command/args/url/env \
                     differ from the approved baseline",
                    pin.config_digest, s.config_digest
                ),
            });
        }
        if !pin.executable_digest.is_empty()
            && !s.executable_digest.is_empty()
            && pin.executable_digest != s.executable_digest
        {
            findings.push(Finding {
                severity: Severity::Critical,
                code: "MCP_EXEC_DRIFT".into(),
                subject: key,
                detail: format!(
                    "server executable changed since it was pinned ({} → {})",
                    pin.executable_digest, s.executable_digest
                ),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, config_digest: &str, exec_digest: &str) -> McpServerRecord {
        McpServerRecord {
            name: name.into(),
            client: "cursor".into(),
            config_digest: config_digest.into(),
            executable_digest: exec_digest.into(),
            ..Default::default()
        }
    }

    #[test]
    fn pin_then_no_drift() {
        let dir = tempfile::tempdir().unwrap();
        let servers = vec![record("files", "aaaa", "bbbb")];
        pin_servers(dir.path(), &servers).unwrap();
        let pins = load(dir.path());
        assert!(detect_drift(&pins, &servers).is_empty());
    }

    #[test]
    fn config_change_flags_drift() {
        let dir = tempfile::tempdir().unwrap();
        pin_servers(dir.path(), &[record("files", "aaaa", "bbbb")]).unwrap();
        let pins = load(dir.path());
        let drifted = vec![record("files", "cccc", "bbbb")];
        let findings = detect_drift(&pins, &drifted);
        assert!(findings.iter().any(|f| f.code == "MCP_CONFIG_DRIFT"));
    }

    #[test]
    fn exec_change_flags_drift() {
        let dir = tempfile::tempdir().unwrap();
        pin_servers(dir.path(), &[record("files", "aaaa", "bbbb")]).unwrap();
        let pins = load(dir.path());
        let findings = detect_drift(&pins, &[record("files", "aaaa", "dddd")]);
        assert!(findings.iter().any(|f| f.code == "MCP_EXEC_DRIFT"));
    }

    #[test]
    fn unpinned_server_not_flagged() {
        let pins = PinSet::default();
        assert!(detect_drift(&pins, &[record("new", "x", "y")]).is_empty());
    }
}
