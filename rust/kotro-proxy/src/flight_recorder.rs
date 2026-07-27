//! Persistent, tamper-evident Agent Flight Recorder — local incident tape for
//! proxied LLM calls, MCP tool activity, hook decisions, and operator actions.
//!
//! Design goals (Phase 0 of the Local Agent Guard plan):
//! - Append-only redb event store bounded by capacity and age.
//! - RFC 3339 timestamps.
//! - Prompt fingerprints use a random per-install HMAC key (not dictionary-testable).
//! - Every event carries `prev_hash`/`hash` forming a verifiable hash chain.
//! - The kill switch state (scope) persists across restarts in the same store.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use rand::RngCore;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DEFAULT_CAPACITY: usize = 200;
/// Events older than this are pruned (7 days).
pub const DEFAULT_MAX_AGE_SECS: u64 = 7 * 24 * 3600;

const EVENTS: TableDefinition<u64, &[u8]> = TableDefinition::new("flight_events");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("flight_meta");

const META_HMAC_KEY: &str = "hmac_key";
const META_KILL_SCOPE: &str = "kill_scope";

/// Which planes a kill switch engagement halts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillScope {
    /// Nothing halted.
    None,
    /// New LLM upstream forwards halted.
    Llm,
    /// MCP tool calls halted (enforced by the action plane / policy checks).
    Tools,
    /// Both planes halted.
    All,
}

impl KillScope {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "llm" => Self::Llm,
            "tools" => Self::Tools,
            "all" => Self::All,
            _ => Self::None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Llm => "llm",
            Self::Tools => "tools",
            Self::All => "all",
        }
    }

    pub fn halts_llm(self) -> bool {
        matches!(self, Self::Llm | Self::All)
    }

    pub fn halts_tools(self) -> bool {
        matches!(self, Self::Tools | Self::All)
    }

    pub fn engaged(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightKind {
    Request,
    CacheHit,
    CacheMiss,
    CircuitOpen,
    ToolLoop,
    ToolStorm,
    RateLimit,
    Budget,
    Injection,
    KillSwitch,
    Observe,
    // Action plane (MCP / hooks)
    ToolDiscovery,
    ToolCall,
    ToolDenied,
    ToolDrift,
    ChainAlert,
    Approval,
    PostureFinding,
}

impl Default for FlightKind {
    fn default() -> Self {
        Self::Observe
    }
}

fn default_plane() -> String {
    "llm".into()
}

/// Canonical cross-plane event schema. LLM proxy events, MCP action-plane
/// events, hook decisions, and operator actions all serialize to this shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlightEvent {
    /// Monotonic sequence number assigned by the recorder.
    #[serde(default)]
    pub seq: u64,
    /// RFC 3339 UTC timestamp.
    #[serde(default)]
    pub at: String,
    /// "llm" | "mcp" | "hook" | "ops".
    #[serde(default = "default_plane")]
    pub plane: String,
    #[serde(default)]
    pub kind: FlightKind,
    /// Session/scope identifier used for cross-plane correlation.
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub route: String,
    /// Tool name for action-plane events.
    #[serde(default)]
    pub tool_name: String,
    /// MCP server name for action-plane events.
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub cache_status: String,
    /// HMAC-keyed prompt-state fingerprint (never a bare hash of the prompt).
    #[serde(default)]
    pub prompt_hash: String,
    #[serde(default)]
    pub estimated_tokens: u64,
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default)]
    pub redaction_count: u32,
    #[serde(default)]
    pub tool_rounds: u32,
    /// Provenance label: trusted_user | trusted_local | untrusted_web |
    /// untrusted_repo | untrusted_tool | unknown.
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub enforced: bool,
    /// Hash of the previous event in the chain (hex).
    #[serde(default)]
    pub prev_hash: String,
    /// Chain hash of this event (hex): SHA-256(prev_hash || canonical body).
    #[serde(default)]
    pub hash: String,
}

impl FlightEvent {
    /// Bytes covered by the chain hash: every field except `hash` itself.
    fn chain_material(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(self.prev_hash.as_bytes());
        let body = serde_json::json!([
            self.seq,
            self.at,
            self.plane,
            self.kind,
            self.session,
            self.provider,
            self.model,
            self.route,
            self.tool_name,
            self.server,
            self.cache_status,
            self.prompt_hash,
            self.estimated_tokens,
            self.latency_ms,
            self.redaction_count,
            self.tool_rounds,
            self.provenance,
            self.detail,
            self.enforced,
        ]);
        out.extend_from_slice(serde_json::to_string(&body).unwrap_or_default().as_bytes());
        out
    }

    fn compute_hash(&self) -> String {
        let digest = Sha256::digest(self.chain_material());
        hex(&digest)
    }
}

/// Partial event accepted from other planes (mcp-wrap, hook adapter) via the
/// authenticated control API. The recorder assigns seq/at/chain fields.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FlightDraft {
    #[serde(default = "default_plane")]
    pub plane: String,
    #[serde(default)]
    pub kind: FlightKind,
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub route: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub cache_status: String,
    #[serde(default)]
    pub prompt_hash: String,
    #[serde(default)]
    pub estimated_tokens: u64,
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default)]
    pub redaction_count: u32,
    #[serde(default)]
    pub tool_rounds: u32,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub enforced: bool,
}

struct Inner {
    ring: VecDeque<FlightEvent>,
    next_seq: u64,
    last_hash: String,
    kill_scope: KillScope,
}

pub struct FlightRecorder {
    enabled: bool,
    capacity: usize,
    max_age_secs: u64,
    hmac_key: [u8; 32],
    inner: Mutex<Inner>,
    db: Option<Database>,
}

impl FlightRecorder {
    /// In-memory-only recorder (tests, or when no state dir is available).
    pub fn new(enabled: bool, capacity: usize) -> Self {
        let mut hmac_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut hmac_key);
        Self {
            enabled,
            capacity: capacity.max(1),
            max_age_secs: DEFAULT_MAX_AGE_SECS,
            hmac_key,
            inner: Mutex::new(Inner {
                ring: VecDeque::new(),
                next_seq: 1,
                last_hash: String::new(),
                kill_scope: KillScope::None,
            }),
            db: None,
        }
    }

    /// Open (or create) the persistent recorder at `db_path`. Existing events,
    /// the per-install HMAC key, and the persisted kill-switch scope are loaded.
    pub fn open(
        enabled: bool,
        capacity: usize,
        max_age_secs: u64,
        db_path: &Path,
    ) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create state dir: {e}"))?;
        }
        let db = Database::create(db_path).map_err(|e| format!("open flight db: {e}"))?;

        // Load or mint the per-install HMAC key and persisted kill scope.
        let mut hmac_key = [0u8; 32];
        let mut have_key = false;
        let mut kill_scope = KillScope::None;
        {
            let read = db.begin_read().map_err(|e| e.to_string())?;
            if let Ok(meta) = read.open_table(META) {
                if let Ok(Some(v)) = meta.get(META_HMAC_KEY) {
                    let bytes = v.value();
                    if bytes.len() == 32 {
                        hmac_key.copy_from_slice(bytes);
                        have_key = true;
                    }
                }
                if let Ok(Some(v)) = meta.get(META_KILL_SCOPE) {
                    kill_scope = KillScope::parse(&String::from_utf8_lossy(v.value()));
                }
            }
        }
        if !have_key {
            rand::thread_rng().fill_bytes(&mut hmac_key);
            let write = db.begin_write().map_err(|e| e.to_string())?;
            {
                let mut meta = write.open_table(META).map_err(|e| e.to_string())?;
                meta.insert(META_HMAC_KEY, hmac_key.as_slice())
                    .map_err(|e| e.to_string())?;
            }
            write.commit().map_err(|e| e.to_string())?;
        }

        // Load the tail of the event log into the in-memory ring.
        let mut ring: VecDeque<FlightEvent> = VecDeque::new();
        let mut next_seq: u64 = 1;
        let mut last_hash = String::new();
        {
            let read = db.begin_read().map_err(|e| e.to_string())?;
            if let Ok(events) = read.open_table(EVENTS) {
                let capacity = capacity.max(1);
                for item in events.iter().map_err(|e| e.to_string())?.rev() {
                    let (k, v) = item.map_err(|e| e.to_string())?;
                    if let Ok(ev) = serde_json::from_slice::<FlightEvent>(v.value()) {
                        if next_seq == 1 {
                            next_seq = k.value() + 1;
                            last_hash = ev.hash.clone();
                        }
                        if ring.len() < capacity {
                            ring.push_back(ev);
                        }
                    }
                }
            }
        }

        Ok(Self {
            enabled,
            capacity: capacity.max(1),
            max_age_secs: max_age_secs.max(60),
            hmac_key,
            inner: Mutex::new(Inner {
                ring,
                next_seq,
                last_hash,
                kill_scope,
            }),
            db: Some(db),
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn persistent(&self) -> bool {
        self.db.is_some()
    }

    /// HMAC-SHA256 prompt fingerprint truncated to 16 hex chars.
    /// Keyed with the random per-install key, so it is not dictionary-testable.
    pub fn prompt_fingerprint(&self, text: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.hmac_key)
            .expect("HMAC accepts any key length");
        mac.update(text.as_bytes());
        let out = mac.finalize().into_bytes();
        hex(&out)[..16].to_string()
    }

    pub fn record(&self, draft: FlightDraft) -> Option<FlightEvent> {
        if !self.enabled {
            return None;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut event = FlightEvent {
            seq: inner.next_seq,
            at: now_rfc3339(),
            plane: draft.plane,
            kind: draft.kind,
            session: draft.session,
            provider: draft.provider,
            model: draft.model,
            route: draft.route,
            tool_name: draft.tool_name,
            server: draft.server,
            cache_status: draft.cache_status,
            prompt_hash: draft.prompt_hash,
            estimated_tokens: draft.estimated_tokens,
            latency_ms: draft.latency_ms,
            redaction_count: draft.redaction_count,
            tool_rounds: draft.tool_rounds,
            provenance: draft.provenance,
            detail: draft.detail,
            enforced: draft.enforced,
            prev_hash: inner.last_hash.clone(),
            hash: String::new(),
        };
        event.hash = event.compute_hash();

        inner.next_seq += 1;
        inner.last_hash = event.hash.clone();
        inner.ring.push_front(event.clone());
        while inner.ring.len() > self.capacity {
            inner.ring.pop_back();
        }
        drop(inner);

        if let Some(db) = &self.db {
            if let Err(e) = self.persist(db, &event) {
                tracing::warn!(error = %e, "flight recorder: persist failed");
            }
        }
        Some(event)
    }

    fn persist(&self, db: &Database, event: &FlightEvent) -> Result<(), String> {
        let bytes = serde_json::to_vec(event).map_err(|e| e.to_string())?;
        let write = db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut events = write.open_table(EVENTS).map_err(|e| e.to_string())?;
            events
                .insert(event.seq, bytes.as_slice())
                .map_err(|e| e.to_string())?;

            // Prune by count (keep a generous multiple of the ring capacity on
            // disk) and by age.
            let disk_cap = (self.capacity as u64) * 50;
            let min_keep_seq = event.seq.saturating_sub(disk_cap);
            let cutoff = now_epoch_secs().saturating_sub(self.max_age_secs);
            let mut prune: Vec<u64> = Vec::new();
            for item in events.iter().map_err(|e| e.to_string())? {
                let (k, v) = item.map_err(|e| e.to_string())?;
                let seq = k.value();
                if seq < min_keep_seq {
                    prune.push(seq);
                    continue;
                }
                if let Ok(ev) = serde_json::from_slice::<FlightEvent>(v.value()) {
                    if let Some(secs) = rfc3339_to_epoch(&ev.at) {
                        if secs < cutoff {
                            prune.push(seq);
                            continue;
                        }
                    }
                }
                break; // events are seq-ordered; the rest are newer
            }
            for seq in prune {
                let _ = events.remove(seq);
            }
        }
        write.commit().map_err(|e| e.to_string())
    }

    /// Newest-first snapshot of up to `limit` events.
    pub fn snapshot(&self, limit: usize) -> Vec<FlightEvent> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.ring.iter().take(limit.max(1)).cloned().collect()
    }

    pub fn export_json(&self) -> String {
        let events = self.snapshot(self.capacity);
        serde_json::to_string_pretty(&events).unwrap_or_else(|_| "[]".into())
    }

    /// Verify the persisted hash chain. Returns the number of verified events.
    /// Detects modified or deleted (mid-chain) events.
    pub fn verify(&self) -> Result<u64, String> {
        let Some(db) = &self.db else {
            return self.verify_ring();
        };
        let read = db.begin_read().map_err(|e| e.to_string())?;
        let events = match read.open_table(EVENTS) {
            Ok(t) => t,
            Err(_) => return Ok(0),
        };
        let mut prev_hash: Option<String> = None;
        let mut prev_seq: Option<u64> = None;
        let mut count = 0u64;
        for item in events.iter().map_err(|e| e.to_string())? {
            let (k, v) = item.map_err(|e| e.to_string())?;
            let ev: FlightEvent = serde_json::from_slice(v.value())
                .map_err(|e| format!("event {} corrupt: {e}", k.value()))?;
            if ev.seq != k.value() {
                return Err(format!("event {}: seq mismatch ({})", k.value(), ev.seq));
            }
            if let Some(ps) = prev_seq {
                if ev.seq != ps + 1 {
                    return Err(format!(
                        "chain gap: event {} follows {} (deletion detected)",
                        ev.seq, ps
                    ));
                }
            }
            if let Some(ph) = &prev_hash {
                if &ev.prev_hash != ph {
                    return Err(format!("event {}: prev_hash mismatch", ev.seq));
                }
            }
            if ev.compute_hash() != ev.hash {
                return Err(format!("event {}: hash mismatch (tampered)", ev.seq));
            }
            prev_hash = Some(ev.hash.clone());
            prev_seq = Some(ev.seq);
            count += 1;
        }
        Ok(count)
    }

    fn verify_ring(&self) -> Result<u64, String> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut count = 0u64;
        let mut prev_hash: Option<String> = None;
        for ev in inner.ring.iter().rev() {
            if let Some(ph) = &prev_hash {
                if &ev.prev_hash != ph {
                    return Err(format!("event {}: prev_hash mismatch", ev.seq));
                }
            }
            if ev.compute_hash() != ev.hash {
                return Err(format!("event {}: hash mismatch", ev.seq));
            }
            prev_hash = Some(ev.hash.clone());
            count += 1;
        }
        Ok(count)
    }

    // ── Kill switch (persisted) ──────────────────────────────────────────────

    pub fn kill_scope(&self) -> KillScope {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .kill_scope
    }

    pub fn set_kill_scope(&self, scope: KillScope) {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.kill_scope = scope;
        }
        if let Some(db) = &self.db {
            let res: Result<(), String> = (|| {
                let write = db.begin_write().map_err(|e| e.to_string())?;
                {
                    let mut meta = write.open_table(META).map_err(|e| e.to_string())?;
                    meta.insert(META_KILL_SCOPE, scope.as_str().as_bytes())
                        .map_err(|e| e.to_string())?;
                }
                write.commit().map_err(|e| e.to_string())
            })();
            if let Err(e) = res {
                tracing::warn!(error = %e, "flight recorder: kill scope persist failed");
            }
        }
    }
}

impl Default for FlightRecorder {
    fn default() -> Self {
        Self::new(true, DEFAULT_CAPACITY)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// RFC 3339 UTC timestamp (millisecond precision) without a chrono dependency.
pub fn now_rfc3339() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    epoch_to_rfc3339(dur.as_secs(), dur.subsec_millis())
}

fn epoch_to_rfc3339(secs: u64, millis: u32) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        millis
    )
}

/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Inverse of `civil_from_days`, used only for age-based pruning.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

fn rfc3339_to_epoch(ts: &str) -> Option<u64> {
    // Expect "YYYY-MM-DDTHH:MM:SS.mmmZ".
    let bytes = ts.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let y: i64 = ts.get(0..4)?.parse().ok()?;
    let m: u32 = ts.get(5..7)?.parse().ok()?;
    let d: u32 = ts.get(8..10)?.parse().ok()?;
    let hh: u64 = ts.get(11..13)?.parse().ok()?;
    let mm: u64 = ts.get(14..16)?.parse().ok()?;
    let ss: u64 = ts.get(17..19)?.parse().ok()?;
    let days = days_from_civil(y, m, d);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// Count assistant→tool rounds in a conversation (OpenAI tool_calls / Anthropic tool_use).
pub fn count_tool_rounds(messages: &[crate::models::unified::UnifiedMessage]) -> u32 {
    let mut rounds = 0u32;
    for msg in messages {
        if msg.role != "assistant" {
            continue;
        }
        if let Some(tool_calls) = &msg.tool_calls {
            if tool_calls.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                rounds = rounds.saturating_add(1);
                continue;
            }
        }
        // Anthropic-style: content blocks may include tool_use in JSON content arrays.
        if let Some(arr) = msg.content.as_array() {
            if arr.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use")) {
                rounds = rounds.saturating_add(1);
            }
        }
    }
    rounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::unified::UnifiedMessage;
    use serde_json::json;

    fn draft(detail: &str) -> FlightDraft {
        FlightDraft {
            plane: "llm".into(),
            kind: FlightKind::Request,
            session: "s1".into(),
            provider: "openai".into(),
            model: "gpt".into(),
            route: "/v1/chat/completions".into(),
            cache_status: "miss".into(),
            detail: detail.into(),
            ..Default::default()
        }
    }

    #[test]
    fn ring_bounds_and_chain() {
        let rec = FlightRecorder::new(true, 2);
        for i in 0..5 {
            rec.record(draft(&format!("e{i}")));
        }
        assert_eq!(rec.snapshot(10).len(), 2);
        assert!(rec.verify_ring().is_ok());
    }

    #[test]
    fn fingerprint_is_keyed_per_install() {
        let a = FlightRecorder::new(true, 4);
        let b = FlightRecorder::new(true, 4);
        let fa = a.prompt_fingerprint("secret prompt");
        let fb = b.prompt_fingerprint("secret prompt");
        assert_eq!(fa.len(), 16);
        // Different install keys → different fingerprints for the same input.
        assert_ne!(fa, fb);
        // Stable within one install.
        assert_eq!(fa, a.prompt_fingerprint("secret prompt"));
    }

    #[test]
    fn rfc3339_shape() {
        let ts = now_rfc3339();
        assert_eq!(ts.len(), 24);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
        // Round-trip through the parser.
        assert!(rfc3339_to_epoch(&ts).is_some());
    }

    #[test]
    fn civil_conversion_roundtrip() {
        // 2026-07-25 00:00:00 UTC = 1784937600
        assert_eq!(epoch_to_rfc3339(1_784_937_600, 0), "2026-07-25T00:00:00.000Z");
        assert_eq!(rfc3339_to_epoch("2026-07-25T00:00:00.000Z"), Some(1_784_937_600));
    }

    #[test]
    fn persists_and_verifies_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flight.redb");
        {
            let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();
            for i in 0..5 {
                rec.record(draft(&format!("e{i}")));
            }
            rec.set_kill_scope(KillScope::Tools);
            assert_eq!(rec.verify().unwrap(), 5);
        }
        // Reopen — events, chain, kill scope, and HMAC key all survive.
        let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();
        assert_eq!(rec.snapshot(100).len(), 5);
        assert_eq!(rec.verify().unwrap(), 5);
        assert_eq!(rec.kill_scope(), KillScope::Tools);
        // Chain continues from the persisted tail.
        rec.record(draft("after-restart"));
        assert_eq!(rec.verify().unwrap(), 6);
    }

    #[test]
    fn hmac_key_stable_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flight.redb");
        let fp1 = {
            let rec = FlightRecorder::open(true, 10, DEFAULT_MAX_AGE_SECS, &path).unwrap();
            rec.prompt_fingerprint("same prompt")
        };
        let rec = FlightRecorder::open(true, 10, DEFAULT_MAX_AGE_SECS, &path).unwrap();
        assert_eq!(fp1, rec.prompt_fingerprint("same prompt"));
    }

    #[test]
    fn tamper_detection_on_modified_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flight.redb");
        {
            let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();
            for i in 0..3 {
                rec.record(draft(&format!("e{i}")));
            }
        }
        // Tamper: rewrite event 2's detail directly in the db.
        {
            let db = Database::create(&path).unwrap();
            let write = db.begin_write().unwrap();
            {
                let mut events = write.open_table(EVENTS).unwrap();
                let raw = events.get(2u64).unwrap().unwrap().value().to_vec();
                let mut ev: FlightEvent = serde_json::from_slice(&raw).unwrap();
                ev.detail = "forged".into();
                let bytes = serde_json::to_vec(&ev).unwrap();
                events.insert(2u64, bytes.as_slice()).unwrap();
            }
            write.commit().unwrap();
        }
        let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();
        let err = rec.verify().unwrap_err();
        assert!(err.contains("hash mismatch"), "got: {err}");
    }

    #[test]
    fn tamper_detection_on_deleted_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flight.redb");
        {
            let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();
            for i in 0..3 {
                rec.record(draft(&format!("e{i}")));
            }
        }
        {
            let db = Database::create(&path).unwrap();
            let write = db.begin_write().unwrap();
            {
                let mut events = write.open_table(EVENTS).unwrap();
                events.remove(2u64).unwrap();
            }
            write.commit().unwrap();
        }
        let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();
        let err = rec.verify().unwrap_err();
        assert!(err.contains("gap") || err.contains("prev_hash"), "got: {err}");
    }

    #[test]
    fn counts_tool_rounds() {
        let msgs = vec![
            UnifiedMessage {
                role: "user".into(),
                content: json!("hi"),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            UnifiedMessage {
                role: "assistant".into(),
                content: json!(""),
                name: None,
                tool_call_id: None,
                tool_calls: Some(json!([{"id":"1","type":"function","function":{"name":"read","arguments":"{}"}}])),
            },
            UnifiedMessage {
                role: "tool".into(),
                content: json!("ok"),
                name: None,
                tool_call_id: Some("1".into()),
                tool_calls: None,
            },
            UnifiedMessage {
                role: "assistant".into(),
                content: json!(""),
                name: None,
                tool_call_id: None,
                tool_calls: Some(json!([{"id":"2","type":"function","function":{"name":"read","arguments":"{}"}}])),
            },
        ];
        assert_eq!(count_tool_rounds(&msgs), 2);
    }
}
