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

/// Chain-hash layout written by this build. Bump only when adding a field to
/// the hashed body, and add a matching `chain_material_vN` — never edit an
/// existing one.
pub const CHAIN_SCHEMA_VERSION: u16 = 2;

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

impl FlightKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::CacheHit => "cache_hit",
            Self::CacheMiss => "cache_miss",
            Self::CircuitOpen => "circuit_open",
            Self::ToolLoop => "tool_loop",
            Self::ToolStorm => "tool_storm",
            Self::RateLimit => "rate_limit",
            Self::Budget => "budget",
            Self::Injection => "injection",
            Self::KillSwitch => "kill_switch",
            Self::Observe => "observe",
            Self::ToolDiscovery => "tool_discovery",
            Self::ToolCall => "tool_call",
            Self::ToolDenied => "tool_denied",
            Self::ToolDrift => "tool_drift",
            Self::ChainAlert => "chain_alert",
            Self::Approval => "approval",
            Self::PostureFinding => "posture_finding",
        }
    }
}



fn default_plane() -> String {
    "llm".into()
}

/// Canonical cross-plane event schema. LLM proxy events, MCP action-plane
/// events, hook decisions, and operator actions all serialize to this shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlightEvent {
    /// Chain-hash schema version. `0` is the pre-trace-context layout and is
    /// what every persisted tape written before this field existed
    /// deserializes to. `1` extends the hashed body with the W3C trace ids.
    ///
    /// The recorder always writes the current version; older values exist only
    /// when reading historical tapes. Never renumber: the value selects which
    /// byte layout `chain_material` reproduces, so changing it retroactively
    /// invalidates the tapes it was meant to preserve.
    #[serde(default)]
    pub schema_version: u16,
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
    /// Task envelope id (empty until Phase 1 signing lands).
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub parent_task_id: String,
    /// Stable decision id for the policy evaluation that produced this event.
    #[serde(default)]
    pub decision_id: String,
    /// Matched rule id when available.
    #[serde(default)]
    pub rule_id: String,
    /// Fingerprint of the effective policy set.
    #[serde(default)]
    pub policy_revision: String,
    #[serde(default)]
    pub tool_call_id: String,
    /// W3C Trace Context ids from MCP `params._meta.traceparent` (SEP-414).
    /// Covered by the chain hash at `schema_version >= 1`. These carry the
    /// correlation to external traces, so leaving them outside the chain would
    /// let an event's attribution be rewritten while the tape still verified.
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub span_id: String,
    /// Principal subject (email / OIDC sub / local user). Chained at v2+.
    #[serde(default)]
    pub principal_subject: String,
    #[serde(default)]
    pub principal_issuer: String,
    #[serde(default)]
    pub agent_name: String,
    #[serde(default)]
    pub agent_instance: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub credential_id: String,
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
        match self.schema_version {
            0 => self.chain_material_v0(),
            1 => self.chain_material_v1(),
            _ => self.chain_material_v2(),
        }
    }

    /// Chain body for `schema_version == 0`.
    ///
    /// COMPATIBILITY ARTIFACT — do not refactor, reorder, or "tidy" this
    /// function. Every byte it produces must stay identical to the layout that
    /// wrote the tapes already on disk; any change silently fails verification
    /// for historical events, which is precisely the property the chain exists
    /// to provide. New fields belong in `chain_material_v1` behind a version
    /// bump, never here.
    fn chain_material_v0(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(self.prev_hash.as_bytes());
        let body = serde_json::json!([
            self.seq,
            self.at,
            self.plane,
            self.kind,
            self.session,
            self.task_id,
            self.parent_task_id,
            self.decision_id,
            self.rule_id,
            self.policy_revision,
            self.tool_call_id,
            self.destination,
            self.credential_id,
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

    /// Chain body for `schema_version == 1`: v0 plus the W3C trace ids.
    ///
    /// The version is hashed first so a v0 and v1 event with otherwise equal
    /// fields cannot collide, and so a downgrade attack that rewrites
    /// `schema_version` to 0 to shed the trace fields changes the digest.
    fn chain_material_v1(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(320);
        out.extend_from_slice(self.prev_hash.as_bytes());
        let body = serde_json::json!([
            self.schema_version,
            self.seq,
            self.at,
            self.plane,
            self.kind,
            self.session,
            self.task_id,
            self.parent_task_id,
            self.decision_id,
            self.rule_id,
            self.policy_revision,
            self.tool_call_id,
            self.trace_id,
            self.span_id,
            self.destination,
            self.credential_id,
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

    /// Chain body for `schema_version == 2`: v1 plus principal/agent identity.
    ///
    /// COMPATIBILITY: do not edit `chain_material_v1`; identity fields land here
    /// behind a version bump so v1 tapes still verify.
    fn chain_material_v2(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(384);
        out.extend_from_slice(self.prev_hash.as_bytes());
        let body = serde_json::json!([
            self.schema_version,
            self.seq,
            self.at,
            self.plane,
            self.kind,
            self.session,
            self.task_id,
            self.parent_task_id,
            self.decision_id,
            self.rule_id,
            self.policy_revision,
            self.tool_call_id,
            self.trace_id,
            self.span_id,
            self.principal_subject,
            self.principal_issuer,
            self.agent_name,
            self.agent_instance,
            self.destination,
            self.credential_id,
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

    /// Export as the stable `kotro-types` KotroEvent v1 shape.
    pub fn to_kotro_event(&self) -> kotro_types::KotroEventV1 {
        kotro_types::KotroEventV1 {
            api_version: kotro_types::EVENT_SCHEMA_VERSION.to_string(),
            seq: self.seq,
            at: self.at.clone(),
            plane: self.plane.clone(),
            kind: self.kind.as_str().to_string(),
            session: self.session.clone(),
            task_id: kotro_types::TaskId::new(self.task_id.clone()),
            parent_task_id: kotro_types::TaskId::new(self.parent_task_id.clone()),
            principal: kotro_types::Principal {
                subject: self.principal_subject.clone(),
                issuer: self.principal_issuer.clone(),
            },
            agent: kotro_types::AgentIdentity {
                name: self.agent_name.clone(),
                instance: self.agent_instance.clone(),
                workload_identity: String::new(),
            },
            decision_id: self.decision_id.clone(),
            rule_id: self.rule_id.clone(),
            policy_revision: self.policy_revision.clone(),
            tool_call_id: self.tool_call_id.clone(),
            trace_id: self.trace_id.clone(),
            span_id: self.span_id.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            route: self.route.clone(),
            tool_name: self.tool_name.clone(),
            server: self.server.clone(),
            destination: self.destination.clone(),
            credential_id: self.credential_id.clone(),
            cache_status: self.cache_status.clone(),
            prompt_hash: self.prompt_hash.clone(),
            estimated_tokens: self.estimated_tokens,
            latency_ms: self.latency_ms,
            redaction_count: self.redaction_count,
            tool_rounds: self.tool_rounds,
            provenance: self.provenance.clone(),
            detail: self.detail.clone(),
            enforced: self.enforced,
            prev_hash: self.prev_hash.clone(),
            hash: self.hash.clone(),
        }
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
    pub task_id: String,
    #[serde(default)]
    pub parent_task_id: String,
    #[serde(default)]
    pub decision_id: String,
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub policy_revision: String,
    #[serde(default)]
    pub tool_call_id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub span_id: String,
    #[serde(default)]
    pub principal_subject: String,
    #[serde(default)]
    pub principal_issuer: String,
    #[serde(default)]
    pub agent_name: String,
    #[serde(default)]
    pub agent_instance: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub credential_id: String,
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
            schema_version: CHAIN_SCHEMA_VERSION,
            seq: inner.next_seq,
            at: now_rfc3339(),
            plane: draft.plane,
            kind: draft.kind,
            session: draft.session,
            task_id: draft.task_id,
            parent_task_id: draft.parent_task_id,
            decision_id: draft.decision_id,
            rule_id: draft.rule_id,
            policy_revision: draft.policy_revision,
            tool_call_id: draft.tool_call_id,
            trace_id: draft.trace_id,
            span_id: draft.span_id,
            principal_subject: draft.principal_subject,
            principal_issuer: draft.principal_issuer,
            agent_name: draft.agent_name,
            agent_instance: draft.agent_instance,
            destination: draft.destination,
            credential_id: draft.credential_id,
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

    /// Chain hashes for a three-event `schema_version: 0` tape, computed
    /// independently of this crate (see the fixture note below). They encode
    /// the exact v0 byte layout, so if `chain_material_v0` is ever edited —
    /// reordered, re-indented into a different JSON shape, a field added — this
    /// stops matching and the test fails. Self-computed expectations could not
    /// catch that, because they would drift along with the code.
    const GOLDEN_V0_HASHES: [&str; 3] = [
        "fb2886740dd9fc2ac45a524bc5c509f4669534fb0ea4e10a2a4993c2bb21cb82",
        "ac003ff32907d00deb0ffd54d07bef1077ed040ec40addca4a5dab476237f0c9",
        "5fc1f761b87e378740d86edebe6d3624f98f25638fb17a9c0ee3de8bd853b454",
    ];

    /// Builds the golden v0 event for `seq`, matching the fixture generator.
    fn golden_v0_event(seq: u64, prev_hash: &str) -> FlightEvent {
        FlightEvent {
            schema_version: 0,
            seq,
            at: "2026-01-15T09:30:00Z".into(),
            plane: "llm".into(),
            kind: FlightKind::Request,
            session: "golden-s1".into(),
            provider: "openai".into(),
            model: "gpt-4o".into(),
            route: "/v1/chat/completions".into(),
            cache_status: "miss".into(),
            prompt_hash: format!("ph{seq}"),
            estimated_tokens: 100,
            latency_ms: 42,
            provenance: "trusted_user".into(),
            detail: format!("golden-{seq}"),
            prev_hash: prev_hash.into(),
            hash: String::new(),
            ..Default::default()
        }
    }

    /// A tape written before `schema_version` existed must still verify.
    ///
    /// This is the load-bearing test for the whole versioning scheme: it is the
    /// only one that fails if v0 material silently changes, and the tamper
    /// evidence claim is worthless if historical tapes stop verifying.
    #[test]
    fn golden_v0_tape_still_verifies() {
        let mut prev = String::new();
        for (i, expected) in GOLDEN_V0_HASHES.iter().enumerate() {
            let ev = golden_v0_event(i as u64, &prev);
            let got = ev.compute_hash();
            assert_eq!(
                &got, expected,
                "v0 chain material changed at seq {i}; historical tapes would \
                 no longer verify. chain_material_v0 must not be modified."
            );
            prev = got;
        }
    }

    /// v0 and v1 events on one tape both verify, and each uses its own layout.
    #[test]
    fn mixed_v0_v1_tape_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flight.redb");

        // Seed two legacy v0 events, then append via the recorder (which
        // writes v1) and confirm the chain is continuous across the boundary.
        // Use a fresh `at` — the golden fixture's January timestamp would be
        // pruned by persist()'s 7-day max-age when the modern event is written.
        let mut prev = String::new();
        let mut legacy = Vec::new();
        for i in 0..2u64 {
            let mut ev = golden_v0_event(i, &prev);
            ev.at = now_rfc3339();
            ev.hash = ev.compute_hash();
            prev = ev.hash.clone();
            legacy.push(ev);
        }
        {
            let db = Database::create(&path).unwrap();
            let write = db.begin_write().unwrap();
            {
                let mut events = write.open_table(EVENTS).unwrap();
                for ev in &legacy {
                    events
                        .insert(ev.seq, serde_json::to_vec(ev).unwrap().as_slice())
                        .unwrap();
                }
            }
            write.commit().unwrap();
        }

        let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();
        let modern = rec.record(draft("modern")).expect("recorded");
        assert_eq!(modern.schema_version, CHAIN_SCHEMA_VERSION, "new events must be written at current chain version");
        assert_eq!(
            modern.prev_hash, legacy[1].hash,
            "v1 event did not chain onto the trailing v0 event"
        );

        match rec.verify() {
            Ok(n) => assert_eq!(n, 3, "expected 3 verified events, got {n}"),
            Err(e) => panic!("mixed v0/v1 tape failed verification: {e}"),
        }
    }

    /// New events are written at the current version and cover the trace ids.
    #[test]
    fn v1_covers_trace_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flight.redb");
        let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();

        let mut d = draft("traced");
        d.trace_id = "4bf92f3577b34da6a3ce929d0e0e4736".into();
        d.span_id = "00f067aa0ba902b7".into();
        let ev = rec.record(d).expect("recorded");

        assert_eq!(ev.schema_version, CHAIN_SCHEMA_VERSION);
        assert_eq!(ev.schema_version, 2);

        // Rewriting trace attribution must break the chain. Before this change
        // the fields sat outside the hash and this mutation verified clean.
        let mut forged = ev.clone();
        forged.trace_id = "00000000000000000000000000000000".into();
        assert_ne!(
            forged.compute_hash(),
            ev.hash,
            "trace_id is not covered by the chain at v1"
        );

        let mut forged_span = ev.clone();
        forged_span.span_id = "0000000000000000".into();
        assert_ne!(forged_span.compute_hash(), ev.hash, "span_id is not covered");
    }

    /// Downgrading `schema_version` must not let an attacker shed the trace
    /// fields while keeping a valid-looking digest.
    #[test]
    fn version_downgrade_breaks_chain() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flight.redb");
        let rec = FlightRecorder::open(true, 50, DEFAULT_MAX_AGE_SECS, &path).unwrap();

        let mut d = draft("traced");
        d.trace_id = "4bf92f3577b34da6a3ce929d0e0e4736".into();
        d.span_id = "00f067aa0ba902b7".into();
        let ev = rec.record(d).expect("recorded");

        let mut downgraded = ev.clone();
        downgraded.schema_version = 0;
        assert_ne!(
            downgraded.compute_hash(),
            ev.hash,
            "version downgrade produced a matching digest — the version must be \
             part of the v1 hashed body"
        );
    }


    #[test]
    fn v2_covers_principal_and_agent() {
        let rec = FlightRecorder::new(true, 10);
        let mut d = draft("id");
        d.principal_subject = "alice".into();
        d.principal_issuer = "https://issuer.example".into();
        d.agent_name = "claude-code".into();
        d.agent_instance = "laptop".into();
        let ev = rec.record(d).unwrap();
        assert_eq!(ev.schema_version, 2);
        let exported = ev.to_kotro_event();
        assert_eq!(exported.principal.subject, "alice");
        assert_eq!(exported.agent.name, "claude-code");

        let mut forged = ev.clone();
        forged.principal_subject = "mallory".into();
        assert_ne!(forged.compute_hash(), ev.hash, "principal must be chained at v2");

        // Downgrade to v1 sheds identity from the digest.
        let mut downgraded = ev.clone();
        downgraded.schema_version = 1;
        assert_ne!(downgraded.compute_hash(), ev.hash);
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


    #[test]
    fn platform_ids_survive_record_and_export() {
        let rec = FlightRecorder::new(true, 10);
        let mut d = draft("denied");
        d.task_id = "task-1".into();
        d.decision_id = "dec-1".into();
        d.rule_id = "deny-x".into();
        d.policy_revision = "rev".into();
        d.destination = "evil.example".into();
        let ev = rec.record(d).unwrap();
        assert_eq!(ev.task_id, "task-1");
        assert_eq!(ev.decision_id, "dec-1");
        assert_eq!(ev.rule_id, "deny-x");
        let exported = ev.to_kotro_event();
        assert_eq!(exported.api_version, kotro_types::EVENT_SCHEMA_VERSION);
        assert_eq!(exported.task_id.as_str(), "task-1");
        assert_eq!(exported.decision_id, "dec-1");
        assert_eq!(exported.destination, "evil.example");
    }

    #[test]
    fn trace_context_survives_record_and_export() {
        let rec = FlightRecorder::new(true, 10);
        let mut d = draft("traced");
        d.trace_id = "4bf92f3577b34da6a3ce929d0e0e4736".into();
        d.span_id = "00f067aa0ba902b7".into();
        let ev = rec.record(d).unwrap();
        assert_eq!(ev.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ev.span_id, "00f067aa0ba902b7");
        let exported = ev.to_kotro_event();
        assert_eq!(exported.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(exported.span_id, "00f067aa0ba902b7");
        // Trace fields must not break the integrity chain.
        assert!(rec.verify_ring().is_ok());
    }
}
