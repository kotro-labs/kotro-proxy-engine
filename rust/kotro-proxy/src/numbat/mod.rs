//! Numbat interoperability — ingest endpoint findings and map high-severity
//! detections into Kotro responses (kill switch / flight events).
//!
//! Numbat is the observe/detect layer; Kotro owns independent enforcement.
//! This adapter does **not** reimplement Numbat rules. It consumes NDJSON
//! records (as emitted to `~/.numbat/records.ndjson` or HTTP delivery) and
//! translates actionable findings into Kotro control-plane actions.
//!
//! Record shape is intentionally tolerant: Numbat's schema is evolving. We
//! accept common field aliases and ignore unknown keys.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::flight_recorder::{FlightDraft, FlightKind, KillScope};

/// A single NDJSON line from Numbat (event, finding, or decision).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NumbatRecord {
    /// `finding` | `event` | `decision` | `indicator` | …
    #[serde(default, rename = "record_type", alias = "type", alias = "kind")]
    pub record_type: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default, alias = "rule", alias = "ruleId")]
    pub rule_id: String,
    #[serde(default, alias = "session_id", alias = "run_id")]
    pub session: String,
    #[serde(default, alias = "message", alias = "title", alias = "description")]
    pub summary: String,
    #[serde(default, alias = "source_agent")]
    pub agent: String,
    #[serde(default, alias = "observed_command")]
    pub tool: String,
    /// Pass-through of the original object for evidence.
    #[serde(default, skip_serializing)]
    pub raw: Option<Value>,
}

impl NumbatRecord {
    pub fn from_json_line(line: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let mut rec: NumbatRecord = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        if rec.record_type.is_empty() {
            // Heuristic: presence of severity ⇒ finding.
            if !rec.severity.is_empty() || v.get("finding").is_some() {
                rec.record_type = "finding".into();
            } else {
                rec.record_type = "event".into();
            }
        }
        rec.raw = Some(v);
        Ok(rec)
    }

    pub fn is_finding(&self) -> bool {
        let t = self.record_type.to_ascii_lowercase();
        t == "finding" || t == "detection" || t == "alert"
    }

    pub fn severity_rank(&self) -> u8 {
        match self.severity.to_ascii_lowercase().as_str() {
            "critical" | "fatal" => 4,
            "high" | "error" => 3,
            "medium" | "warn" | "warning" => 2,
            "low" | "info" | "informational" => 1,
            _ => 0,
        }
    }

    /// Whether Kotro should engage an enforcement response.
    pub fn actionable(&self) -> bool {
        self.is_finding() && self.severity_rank() >= 3
    }
}

/// Suggested Kotro response for one finding.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NumbatResponseAction {
    None,
    /// Engage tools-scope kill switch.
    KillTools,
    /// Engage all-scope kill switch.
    KillAll,
}

#[derive(Debug, Clone, Serialize)]
pub struct NumbatIngestResult {
    pub accepted: usize,
    pub findings: usize,
    pub actionable: usize,
    pub action: NumbatResponseAction,
    pub kill_scope: String,
    pub sessions: Vec<String>,
    pub rule_ids: Vec<String>,
}

/// Parse a multi-line NDJSON blob and decide the strongest response.
pub fn evaluate_ndjson(body: &str) -> (Vec<NumbatRecord>, NumbatIngestResult) {
    let mut records = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(rec) = NumbatRecord::from_json_line(line) {
            records.push(rec);
        }
    }
    let findings: Vec<_> = records.iter().filter(|r| r.is_finding()).cloned().collect();
    let actionable: Vec<_> = findings.iter().filter(|r| r.actionable()).cloned().collect();

    let action = if actionable.iter().any(|r| r.severity_rank() >= 4) {
        NumbatResponseAction::KillAll
    } else if !actionable.is_empty() {
        NumbatResponseAction::KillTools
    } else {
        NumbatResponseAction::None
    };

    let kill_scope = match action {
        NumbatResponseAction::KillAll => KillScope::All,
        NumbatResponseAction::KillTools => KillScope::Tools,
        NumbatResponseAction::None => KillScope::None,
    };

    let mut sessions: Vec<String> = actionable
        .iter()
        .map(|r| r.session.clone())
        .filter(|s| !s.is_empty())
        .collect();
    sessions.sort();
    sessions.dedup();

    let mut rule_ids: Vec<String> = actionable.iter().map(|r| r.rule_id.clone()).collect();
    rule_ids.sort();
    rule_ids.dedup();

    let result = NumbatIngestResult {
        accepted: records.len(),
        findings: findings.len(),
        actionable: actionable.len(),
        action,
        kill_scope: kill_scope.as_str().to_string(),
        sessions,
        rule_ids,
    };
    (records, result)
}

/// Build flight drafts that document the ingest for the tape.
pub fn flight_drafts_for(records: &[NumbatRecord], result: &NumbatIngestResult) -> Vec<FlightDraft> {
    let mut out = Vec::new();
    for rec in records.iter().filter(|r| r.actionable()) {
        out.push(FlightDraft {
            plane: "ops".into(),
            kind: FlightKind::ChainAlert,
            session: if rec.session.is_empty() {
                "numbat".into()
            } else {
                rec.session.clone()
            },
            tool_name: rec.tool.clone(),
            rule_id: rec.rule_id.clone(),
            detail: format!(
                "numbat finding severity={} rule={} — {}",
                rec.severity, rec.rule_id, rec.summary
            ),
            provenance: "numbat".into(),
            enforced: result.action != NumbatResponseAction::None,
            ..Default::default()
        });
    }
    if result.action != NumbatResponseAction::None {
        out.push(FlightDraft {
            plane: "ops".into(),
            kind: FlightKind::KillSwitch,
            session: result.sessions.first().cloned().unwrap_or_else(|| "numbat".into()),
            detail: format!(
                "numbat ingest engaged kill scope {} ({} actionable findings)",
                result.kill_scope, result.actionable
            ),
            provenance: "numbat".into(),
            enforced: true,
            rule_id: result.rule_ids.first().cloned().unwrap_or_default(),
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_finding_and_maps_high_to_kill_tools() {
        let body = r#"
{"type":"event","session":"s1","summary":"noise"}
{"type":"finding","severity":"high","rule_id":"exfil.http","session":"s1","summary":"POST to unknown host","tool":"Bash"}
"#;
        let (recs, result) = evaluate_ndjson(body);
        assert_eq!(recs.len(), 2);
        assert_eq!(result.findings, 1);
        assert_eq!(result.actionable, 1);
        assert_eq!(result.action, NumbatResponseAction::KillTools);
        assert_eq!(result.kill_scope, "tools");
    }

    #[test]
    fn critical_maps_to_kill_all() {
        let body = r#"{"type":"finding","severity":"critical","rule":"credential.exfil","session":"s2","message":"aws key outbound"}"#;
        let (_, result) = evaluate_ndjson(body);
        assert_eq!(result.action, NumbatResponseAction::KillAll);
        assert_eq!(result.kill_scope, "all");
        let drafts = flight_drafts_for(&evaluate_ndjson(body).0, &result);
        assert!(drafts.iter().any(|d| matches!(d.kind, FlightKind::KillSwitch)));
    }

    #[test]
    fn parses_real_numbat_shaped_finding() {
        let body = r#"{"record_type":"finding","severity":"high","rule_id":"chain.secret_read_then_egress","session_id":"readme-live-sequence-01","title":"Secret-file access followed by data-bearing egress","source_agent":"claude-code"}"#;
        let (recs, result) = evaluate_ndjson(body);
        assert_eq!(recs[0].session, "readme-live-sequence-01");
        assert_eq!(result.action, NumbatResponseAction::KillTools);
    }

    #[test]
    fn low_severity_is_not_actionable() {
        let body = r#"{"type":"finding","severity":"low","rule_id":"info","session":"s3"}"#;
        let (_, result) = evaluate_ndjson(body);
        assert_eq!(result.action, NumbatResponseAction::None);
        assert_eq!(result.actionable, 0);
    }
}
