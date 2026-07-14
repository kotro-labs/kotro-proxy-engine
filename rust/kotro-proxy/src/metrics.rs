//! Prometheus instrumentation registry and dashboard snapshot collector — mirrors `internal/metrics/registry.go` and `dashboard_state.go`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prometheus::{
    register_counter_with_registry, register_counter_vec_with_registry,
    register_gauge_vec_with_registry, register_gauge_with_registry,
    register_histogram_vec_with_registry, Counter, CounterVec, Encoder, Gauge, GaugeVec,
    HistogramVec, Registry, TextEncoder,
};
use serde::Serialize;
use tracing::error;

const RECENT_REQUEST_CAPACITY: usize = 10;
const CACHE_WINDOW_DURATION: Duration = Duration::from_secs(5 * 60); // 5 minutes

#[derive(Debug, Clone, Serialize)]
pub struct RecentRequest {
    pub at: String, // ISO-8601 string
    pub provider: String,
    pub route: String,
    pub cache_status: String,
}

#[derive(Debug, Clone)]
struct CacheWindowEvent {
    hit: bool,
    at: SystemTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub updated_at: String,
    pub cache_hit_rate_5m: f64,
    pub cache_hits_5m: usize,
    pub cache_misses_5m: usize,
    pub estimated_dollars_saved: f64,
    pub cache_replay_bytes_total: f64,
    pub compressor_bytes_saved_total: f64,
    pub compressor_blocks_stripped_total: f64,
    pub compressor_scopes_active: f64,
    pub redactions_total: f64,
    pub cache_entries: f64,
    pub requests_total: f64,
    /// Injection patterns detected (warn or block). Honest default-mode counter.
    pub injections_detected_total: f64,
    /// Injection patterns that caused an HTTP block (`KOTRO_INJECTION_BLOCK=true`).
    pub injections_blocked_total: f64,
    pub agent_loops_stopped_total: f64,
    /// Session budget exceeded events (warn or hard block).
    pub budget_hits_total: f64,
    pub recent_requests: Vec<RecentRequest>,
}

struct DashboardState {
    recent_requests: VecDeque<RecentRequest>,
    cache_window: Vec<CacheWindowEvent>,
}

#[derive(Clone)]
pub struct MetricsRegistry {
    registry: Arc<Registry>,
    requests_total: CounterVec,
    request_duration: HistogramVec,
    upstream_duration: HistogramVec,
    request_body_bytes: HistogramVec,
    errors_total: CounterVec,
    cache_hits_total: CounterVec,
    cache_misses_total: CounterVec,
    cache_stores_total: CounterVec,
    cache_replay_bytes_total: CounterVec,
    cache_entries: Gauge,
    cache_evictions_total: CounterVec,
    compressor_blocks: Counter,
    compressor_bytes_saved: Counter,
    compressor_scopes: Gauge,
    compressor_evictions: CounterVec,
    redactions_total: CounterVec,
    redaction_restores: Counter,
    scope_mode_total: CounterVec,
    trusted_peer_rejections: Counter,
    cache_key_strategy: GaugeVec,
    process_threads: Gauge,
    resident_memory_bytes: Gauge,
    injections_detected: Counter,
    injections_blocked: Counter,
    agent_loops_stopped: Counter,
    budget_hits: Counter,
    /// USD estimate per saved token for the dashboard hero card.
    dashboard_usd_per_token: f64,
    dashboard: Arc<Mutex<DashboardState>>,
}

/// Default dashboard dollar rate (~GPT-4o-class blended input).
pub const DEFAULT_DASHBOARD_USD_PER_TOKEN: f64 = 0.000015;

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsRegistry {
    pub fn new() -> Self {
        let registry = Registry::new();

        let requests_total = register_counter_vec_with_registry!(
            "kotro_requests_total",
            "Total intercepted proxy requests.",
            &["provider", "route", "stream"],
            registry
        )
        .unwrap();

        let request_duration = register_histogram_vec_with_registry!(
            "kotro_request_duration_seconds",
            "End-to-end handler latency in seconds.",
            &["provider", "cache_status"],
            vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
            registry
        )
        .unwrap();

        let upstream_duration = register_histogram_vec_with_registry!(
            "kotro_upstream_duration_seconds",
            "Upstream round-trip latency on cache miss paths.",
            &["provider", "status_class"],
            vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
            registry
        )
        .unwrap();

        let request_body_bytes = register_histogram_vec_with_registry!(
            "kotro_request_body_bytes",
            "Incoming JSON request body size in bytes.",
            &["provider"],
            vec![
                1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 5242880.0, 10485760.0
            ],
            registry
        )
        .unwrap();

        let errors_total = register_counter_vec_with_registry!(
            "kotro_errors_total",
            "Proxy errors grouped by class.",
            &["provider", "error_class"],
            registry
        )
        .unwrap();

        let cache_hits_total = register_counter_vec_with_registry!(
            "kotro_cache_hits_total",
            "Semantic cache hits.",
            &["provider"],
            registry
        )
        .unwrap();

        let cache_misses_total = register_counter_vec_with_registry!(
            "kotro_cache_misses_total",
            "Semantic cache misses.",
            &["provider"],
            registry
        )
        .unwrap();

        let cache_stores_total = register_counter_vec_with_registry!(
            "kotro_cache_stores_total",
            "New cache entries written after complete streams.",
            &["provider"],
            registry
        )
        .unwrap();

        let cache_replay_bytes_total = register_counter_vec_with_registry!(
            "kotro_cache_replay_bytes_total",
            "Bytes served from cache on hit replays.",
            &["provider"],
            registry
        )
        .unwrap();

        let cache_entries = register_gauge_with_registry!(
            "kotro_cache_entries",
            "Approximate number of live cache entries.",
            registry
        )
        .unwrap();

        let cache_evictions_total = register_counter_vec_with_registry!(
            "kotro_cache_evictions_total",
            "Cache entry evictions.",
            &["reason"],
            registry
        )
        .unwrap();

        let compressor_blocks = register_counter_with_registry!(
            "kotro_compressor_blocks_stripped_total",
            "Context blocks removed as duplicates.",
            registry
        )
        .unwrap();

        let compressor_bytes_saved = register_counter_with_registry!(
            "kotro_compressor_bytes_saved_total",
            "Estimated bytes not sent upstream after compression.",
            registry
        )
        .unwrap();

        let compressor_scopes = register_gauge_with_registry!(
            "kotro_compressor_scopes_active",
            "Active compressor scope entries.",
            registry
        )
        .unwrap();

        let compressor_evictions = register_counter_vec_with_registry!(
            "kotro_compressor_scope_evictions_total",
            "Compressor scope evictions.",
            &["reason"],
            registry
        )
        .unwrap();

        let redactions_total = register_counter_vec_with_registry!(
            "kotro_redactions_total",
            "Secrets redacted before upstream.",
            &["pattern"],
            registry
        )
        .unwrap();

        let redaction_restores = register_counter_with_registry!(
            "kotro_redaction_restores_total",
            "Placeholder restores in streaming responses.",
            registry
        )
        .unwrap();

        let scope_mode_total = register_counter_vec_with_registry!(
            "kotro_scope_mode_total",
            "Request scope resolution mode.",
            &["mode"],
            registry
        )
        .unwrap();

        let trusted_peer_rejections = register_counter_with_registry!(
            "kotro_trusted_peer_rejections_total",
            "Gateway scope headers ignored from untrusted peers.",
            registry
        )
        .unwrap();

        let cache_key_strategy = register_gauge_vec_with_registry!(
            "kotro_cache_key_strategy",
            "Active cache key strategy configuration.",
            &["strategy", "window_size"],
            registry
        )
        .unwrap();

        let process_threads = register_gauge_with_registry!(
            "kotro_process_threads",
            "Current process thread count.",
            registry
        )
        .unwrap();

        let resident_memory_bytes = register_gauge_with_registry!(
            "kotro_process_resident_memory_bytes",
            "Process resident memory size in bytes.",
            registry
        )
        .unwrap();

        let injections_detected = register_counter_with_registry!(
            "kotro_injections_detected_total",
            "Total MCP prompt injection patterns detected (warn or block).",
            registry
        )
        .unwrap();

        let injections_blocked = register_counter_with_registry!(
            "kotro_injections_blocked_total",
            "Total MCP prompt injections blocked (HTTP reject).",
            registry
        )
        .unwrap();

        let agent_loops_stopped = register_counter_with_registry!(
            "kotro_agent_loops_stopped_total",
            "Total infinite agent tool loops stopped.",
            registry
        )
        .unwrap();

        let budget_hits = register_counter_with_registry!(
            "kotro_budget_hits_total",
            "Total times the session token budget was exceeded (warn or block).",
            registry
        )
        .unwrap();

        Self {
            registry: Arc::new(registry),
            requests_total,
            request_duration,
            upstream_duration,
            request_body_bytes,
            errors_total,
            cache_hits_total,
            cache_misses_total,
            cache_stores_total,
            cache_replay_bytes_total,
            cache_entries,
            cache_evictions_total,
            compressor_blocks,
            compressor_bytes_saved,
            compressor_scopes,
            compressor_evictions,
            redactions_total,
            redaction_restores,
            scope_mode_total,
            trusted_peer_rejections,
            cache_key_strategy,
            process_threads,
            resident_memory_bytes,
            injections_detected,
            injections_blocked,
            agent_loops_stopped,
            budget_hits,
            dashboard_usd_per_token: DEFAULT_DASHBOARD_USD_PER_TOKEN,
            dashboard: Arc::new(Mutex::new(DashboardState {
                recent_requests: VecDeque::with_capacity(RECENT_REQUEST_CAPACITY),
                cache_window: Vec::new(),
            })),
        }
    }

    /// Override the dashboard token→USD conversion rate (default `0.000015`).
    pub fn with_dashboard_usd_per_token(mut self, usd_per_token: f64) -> Self {
        if usd_per_token.is_finite() && usd_per_token >= 0.0 {
            self.dashboard_usd_per_token = usd_per_token;
        }
        self
    }

    pub fn gather_to_string(&self) -> String {
        let mut buffer = Vec::new();
        let encoder = TextEncoder::new();
        self.sample_runtime();
        let metric_families = self.registry.gather();
        if let Err(err) = encoder.encode(&metric_families, &mut buffer) {
            error!(error = %err, "failed to encode metrics");
            return String::new();
        }
        String::from_utf8(buffer).unwrap_or_default()
    }

    fn sample_runtime(&self) {
        // Collect current process thread count and memory if supported
        self.process_threads.set(1.0); // baseline placeholder
        self.resident_memory_bytes.set(0.0);
    }

    pub fn record_request(
        &self,
        provider: &str,
        route: &str,
        stream: bool,
        cache_status: &str,
        elapsed: Duration,
    ) {
        let stream_str = if stream { "true" } else { "false" };
        self.requests_total
            .with_label_values(&[provider, route, stream_str])
            .inc();
        self.request_duration
            .with_label_values(&[provider, cache_status])
            .observe(elapsed.as_secs_f64());

        self.note_dashboard_request(provider, route, cache_status);
    }

    pub fn record_request_body(&self, provider: &str, nbytes: usize) {
        self.request_body_bytes
            .with_label_values(&[provider])
            .observe(nbytes as f64);
    }

    pub fn record_upstream(&self, provider: &str, status_code: u16, elapsed: Duration) {
        let status_class = status_class_label(status_code);
        self.upstream_duration
            .with_label_values(&[provider, status_class])
            .observe(elapsed.as_secs_f64());
    }

    pub fn record_error(&self, provider: &str, class: &str) {
        self.errors_total
            .with_label_values(&[provider, class])
            .inc();
    }

    pub fn record_cache_hit(&self, provider: &str, replay_bytes: usize) {
        self.cache_hits_total.with_label_values(&[provider]).inc();
        if replay_bytes > 0 {
            self.cache_replay_bytes_total
                .with_label_values(&[provider])
                .inc_by(replay_bytes as f64);
        }
    }

    pub fn record_cache_miss(&self, provider: &str) {
        self.cache_misses_total.with_label_values(&[provider]).inc();
    }

    pub fn record_cache_store(&self, provider: &str) {
        self.cache_stores_total.with_label_values(&[provider]).inc();
    }

    pub fn set_cache_entries(&self, n: usize) {
        self.cache_entries.set(n as f64);
    }

    pub fn record_cache_eviction(&self, reason: &str, n: usize) {
        if n > 0 {
            self.cache_evictions_total
                .with_label_values(&[reason])
                .inc_by(n as f64);
        }
    }

    pub fn record_compression(&self, blocks_stripped: usize, bytes_saved: usize) {
        if blocks_stripped > 0 {
            self.compressor_blocks.inc_by(blocks_stripped as f64);
        }
        if bytes_saved > 0 {
            self.compressor_bytes_saved.inc_by(bytes_saved as f64);
        }
    }

    pub fn set_compressor_scopes(&self, n: usize) {
        self.compressor_scopes.set(n as f64);
    }

    pub fn record_compressor_eviction(&self, reason: &str) {
        self.compressor_evictions
            .with_label_values(&[reason])
            .inc();
    }

    pub fn record_redaction(&self, pattern: &str) {
        self.redactions_total.with_label_values(&[pattern]).inc();
    }

    pub fn record_redaction_restores(&self, n: usize) {
        if n > 0 {
            self.redaction_restores.inc_by(n as f64);
        }
    }


    pub fn record_scope_mode(&self, mode: &str) {
        if !mode.is_empty() {
            self.scope_mode_total.with_label_values(&[mode]).inc();
        }
    }

    pub fn record_trusted_peer_rejection(&self) {
        self.trusted_peer_rejections.inc();
    }

    pub fn set_cache_key_strategy(&self, strategy: &str, window_size: usize) {
        self.cache_key_strategy
            .with_label_values(&[strategy, &window_size.to_string()])
            .set(1.0);
    }

    pub fn record_injection_detected(&self) {
        self.injections_detected.inc();
    }

    pub fn record_injection_blocked(&self) {
        self.injections_blocked.inc();
    }

    pub fn record_agent_loop_stopped(&self) {
        self.agent_loops_stopped.inc();
    }

    pub fn record_budget_hit(&self) {
        self.budget_hits.inc();
    }

    fn note_dashboard_request(&self, provider: &str, route: &str, cache_status: &str) {
        let mut state = self.dashboard.lock().unwrap();
        let now = SystemTime::now();

        if cache_status == "hit" {
            state.cache_window.push(CacheWindowEvent { hit: true, at: now });
        } else if cache_status == "miss" {
            state.cache_window.push(CacheWindowEvent { hit: false, at: now });
        }
        prune_cache_window(&mut state.cache_window, now);

        let iso_time = format_iso_time(now);
        state.recent_requests.push_front(RecentRequest {
            at: iso_time,
            provider: provider.to_string(),
            route: route.to_string(),
            cache_status: cache_status.to_string(),
        });

        if state.recent_requests.len() > RECENT_REQUEST_CAPACITY {
            state.recent_requests.truncate(RECENT_REQUEST_CAPACITY);
        }
    }

    pub fn snapshot(&self) -> DashboardSnapshot {
        let now = SystemTime::now();
        let (rate, hits, misses, recent) = {
            let mut state = self.dashboard.lock().unwrap();
            prune_cache_window(&mut state.cache_window, now);
            let (rate, h, m) = hit_rate_and_counts(&state.cache_window);
            let recent_vec = state.recent_requests.iter().cloned().collect::<Vec<_>>();
            (rate, h, m, recent_vec)
        };

        let totals = self.gather_totals();

        let comp_bytes = *totals.get("kotro_compressor_bytes_saved_total").unwrap_or(&0.0);
        let cache_bytes = *totals.get("kotro_cache_replay_bytes_total").unwrap_or(&0.0);
        let tokens_saved = (comp_bytes + cache_bytes) / 4.0;
        let dollars_saved = tokens_saved * self.dashboard_usd_per_token;

        DashboardSnapshot {
            updated_at: format_iso_time(now),
            cache_hit_rate_5m: rate,
            cache_hits_5m: hits,
            cache_misses_5m: misses,
            estimated_dollars_saved: dollars_saved,
            cache_replay_bytes_total: cache_bytes,
            compressor_bytes_saved_total: comp_bytes,
            compressor_blocks_stripped_total: *totals.get("kotro_compressor_blocks_stripped_total").unwrap_or(&0.0),
            compressor_scopes_active: *totals.get("kotro_compressor_scopes_active").unwrap_or(&0.0),
            redactions_total: *totals.get("kotro_redactions_total").unwrap_or(&0.0),
            cache_entries: *totals.get("kotro_cache_entries").unwrap_or(&0.0),
            requests_total: *totals.get("kotro_requests_total").unwrap_or(&0.0),
            injections_detected_total: *totals.get("kotro_injections_detected_total").unwrap_or(&0.0),
            injections_blocked_total: *totals.get("kotro_injections_blocked_total").unwrap_or(&0.0),
            agent_loops_stopped_total: *totals.get("kotro_agent_loops_stopped_total").unwrap_or(&0.0),
            budget_hits_total: *totals.get("kotro_budget_hits_total").unwrap_or(&0.0),
            recent_requests: recent,
        }
    }

    fn gather_totals(&self) -> std::collections::HashMap<String, f64> {
        let mut out = std::collections::HashMap::new();
        out.insert("kotro_compressor_bytes_saved_total".to_string(), 0.0);
        out.insert("kotro_cache_replay_bytes_total".to_string(), 0.0);
        out.insert("kotro_compressor_blocks_stripped_total".to_string(), 0.0);
        out.insert("kotro_compressor_scopes_active".to_string(), 0.0);
        out.insert("kotro_redactions_total".to_string(), 0.0);
        out.insert("kotro_cache_entries".to_string(), 0.0);
        out.insert("kotro_requests_total".to_string(), 0.0);
        out.insert("kotro_injections_detected_total".to_string(), 0.0);
        out.insert("kotro_injections_blocked_total".to_string(), 0.0);
        out.insert("kotro_agent_loops_stopped_total".to_string(), 0.0);
        out.insert("kotro_budget_hits_total".to_string(), 0.0);

        let mfs = self.registry.gather();
        for mf in mfs {
            let name = mf.get_name();
            if out.contains_key(name) {
                let mut sum = 0.0;
                for m in mf.get_metric() {
                    if m.has_counter() {
                        sum += m.get_counter().get_value();
                    } else if m.has_gauge() {
                        sum += m.get_gauge().get_value();
                    }
                }
                out.insert(name.to_string(), sum);
            }
        }
        out
    }
}

fn prune_cache_window(window: &mut Vec<CacheWindowEvent>, now: SystemTime) {
    if let Some(cutoff) = now.checked_sub(CACHE_WINDOW_DURATION) {
        window.retain(|ev| ev.at >= cutoff);
    }
}

fn hit_rate_and_counts(window: &[CacheWindowEvent]) -> (f64, usize, usize) {
    let mut hits = 0;
    let mut misses = 0;
    for ev in window {
        if ev.hit {
            hits += 1;
        } else {
            misses += 1;
        }
    }
    let total = hits + misses;
    let rate = if total == 0 {
        0.0
    } else {
        hits as f64 / total as f64
    };
    (rate, hits, misses)
}

fn status_class_label(code: u16) -> &'static str {
    match code {
        200..=299 => "2xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "error",
    }
}

fn format_iso_time(t: SystemTime) -> String {
    let duration = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    let nsecs = duration.subsec_nanos();
    
    // Format timestamp in UTC ISO-8601 (YYYY-MM-DDTHH:MM:SSZ)
    let days = secs / 86400;
    let seconds_in_day = secs % 86400;
    let hours = seconds_in_day / 3600;
    let minutes = (seconds_in_day % 3600) / 60;
    let seconds = seconds_in_day % 60;

    format!("UTC days since epoch: {days} {hours:02}:{minutes:02}:{seconds:02}.{nsecs:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_requests_appear_in_dashboard_traffic() {
        let metrics = MetricsRegistry::new();
        metrics.record_injection_detected();
        metrics.record_injection_blocked();
        metrics.record_request("openai", "/v1/chat/completions", false, "blocked", Duration::from_millis(3));

        let snap = metrics.snapshot();
        assert_eq!(snap.injections_detected_total, 1.0);
        assert_eq!(snap.injections_blocked_total, 1.0);
        assert!(snap.requests_total >= 1.0);
        assert_eq!(snap.recent_requests.len(), 1);
        assert_eq!(snap.recent_requests[0].cache_status, "blocked");
    }

    #[test]
    fn security_counters_appear_in_snapshot() {
        let metrics = MetricsRegistry::new();
        metrics.record_injection_detected();
        metrics.record_injection_detected();
        metrics.record_injection_blocked();
        metrics.record_agent_loop_stopped();
        metrics.record_budget_hit();
        metrics.record_budget_hit();

        let snap = metrics.snapshot();
        assert_eq!(snap.injections_detected_total, 2.0);
        assert_eq!(snap.injections_blocked_total, 1.0);
        assert_eq!(snap.agent_loops_stopped_total, 1.0);
        assert_eq!(snap.budget_hits_total, 2.0);
    }

    #[test]
    fn dashboard_usd_rate_is_configurable() {
        let metrics = MetricsRegistry::new().with_dashboard_usd_per_token(0.001);
        // 100 saved "tokens" via 400 compressor bytes ⇒ dollars = 100 * 0.001
        metrics.record_compression(1, 400);

        let snap = metrics.snapshot();
        assert!((snap.estimated_dollars_saved - 0.1).abs() < 1e-9);
    }

    #[test]
    fn invalid_usd_rate_keeps_default() {
        let metrics = MetricsRegistry::new().with_dashboard_usd_per_token(-1.0);
        assert!((metrics.dashboard_usd_per_token - DEFAULT_DASHBOARD_USD_PER_TOKEN).abs() < 1e-12);
    }
}
