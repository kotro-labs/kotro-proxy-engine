//! Shared agent governance helpers for route handlers.

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use parking_lot::Mutex;
use reqwest::header::CONTENT_TYPE;
use serde::Serialize;

use crate::flight_recorder::{count_tool_rounds, FlightDraft, FlightEvent, FlightKind, KillScope};
use crate::models::unified::UnifiedRequest;
use crate::router::AppState;

/// Per-session token-bucket rate limiter. Each session (scope key) gets its
/// own bucket, so one runaway agent cannot starve — or be masked by — another.
pub struct SessionRateLimiter {
    buckets: moka::sync::Cache<String, std::sync::Arc<Mutex<Bucket>>>,
    /// Bucket capacity == max requests per minute. 0 = unlimited.
    per_minute: u32,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl SessionRateLimiter {
    pub fn new(per_minute: u32) -> Self {
        Self {
            buckets: moka::sync::Cache::builder()
                .max_capacity(10_000)
                .time_to_idle(Duration::from_secs(600))
                .build(),
            per_minute,
        }
    }

    /// Try to take one token for `session`. `Ok(())` = allowed.
    /// `Err(rate)` = over the limit, where `rate` is the configured per-minute cap.
    pub fn try_acquire(&self, session: &str) -> Result<(), u32> {
        if self.per_minute == 0 {
            return Ok(());
        }
        let cap = self.per_minute as f64;
        let bucket = self.buckets.get_with(session.to_string(), || {
            std::sync::Arc::new(Mutex::new(Bucket {
                tokens: cap,
                last_refill: Instant::now(),
            }))
        });
        let mut b = bucket.lock();
        let elapsed = b.last_refill.elapsed().as_secs_f64();
        b.tokens = (b.tokens + elapsed * cap / 60.0).min(cap);
        b.last_refill = Instant::now();
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            Ok(())
        } else {
            Err(self.per_minute)
        }
    }
}

#[derive(Serialize)]
struct ProblemDetails {
    #[serde(rename = "type")]
    problem_type: String,
    title: String,
    status: u16,
    detail: String,
}

fn problem_response(status: StatusCode, title: &str, detail: &str) -> Response {
    let pd = ProblemDetails {
        problem_type: "https://github.com/kotro-labs/kotro-proxy-engine#errors".into(),
        title: title.to_string(),
        status: status.as_u16(),
        detail: detail.to_string(),
    };
    let mut response = Json(pd).into_response();
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/problem+json"));
    response
}

fn try_set_header(resp: &mut Response, name: &str, value: &str) {
    if let (Ok(n), Ok(v)) = (
        HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_str(value),
    ) {
        resp.headers_mut().insert(n, v);
    }
}

/// Record an LLM-plane flight event. Prompt fingerprints are HMAC-keyed by the
/// recorder; tool rounds are derived from the conversation.
#[allow(clippy::too_many_arguments)]
pub fn record_flight(
    state: &AppState,
    kind: FlightKind,
    provider: &str,
    model: &str,
    route: &str,
    cache_status: &str,
    unified: Option<&UnifiedRequest>,
    session: &str,
    estimated_tokens: u64,
    latency: Duration,
    redaction_count: u32,
    detail: impl Into<String>,
    enforced: bool,
) {
    if !state.flight_recorder.enabled() {
        return;
    }
    let (hash, tool_rounds) = if let Some(u) = unified {
        let mut blob = u.system_prompt.clone();
        for m in &u.messages {
            blob.push('\n');
            blob.push_str(&crate::models::unified::content_text(&m.content));
        }
        (
            state.flight_recorder.prompt_fingerprint(&blob),
            count_tool_rounds(&u.messages),
        )
    } else {
        (String::new(), 0)
    };
    let event = state.flight_recorder.record(FlightDraft {
        plane: "llm".into(),
        kind,
        session: session.into(),
        provider: provider.into(),
        model: model.into(),
        route: route.into(),
        cache_status: cache_status.into(),
        prompt_hash: hash,
        estimated_tokens,
        latency_ms: latency.as_millis() as u64,
        redaction_count,
        tool_rounds,
        detail: detail.into(),
        enforced,
        ..Default::default()
    });
    // GenAI OTel span — identifiers only, never prompt content.
    crate::telemetry::genai::record_llm_span(
        session,
        provider,
        model,
        estimated_tokens,
        0,
        latency.as_millis() as u64,
    );
    if let Some(event) = event {
        correlate(state, &event);
    }
}

/// Feed a freshly recorded event through the cross-plane session graph.
/// Chain alerts are written back to the flight recorder as `ChainAlert`
/// events; critical chains auto-engage the tools kill switch when the kill
/// switch mode enforces and `KOTRO_CHAIN_AUTO_KILL` is not disabled.
pub fn correlate(state: &AppState, event: &FlightEvent) {
    let alerts = state.graph.observe(event);
    for alert in alerts {
        let critical = alert.severity == "critical";
        let auto_kill =
            critical && state.chain_auto_kill && state.kill_switch_mode.enforces();
        tracing::warn!(
            rule = %alert.rule,
            session = %alert.session,
            severity = %alert.severity,
            auto_kill,
            "chain alert: {}",
            alert.evidence
        );
        state.flight_recorder.record(FlightDraft {
            plane: "graph".into(),
            kind: FlightKind::ChainAlert,
            session: alert.session.clone(),
            tool_name: event.tool_name.clone(),
            server: event.server.clone(),
            route: alert.rule.clone(),
            detail: format!("[{}] {}", alert.severity, alert.evidence),
            enforced: auto_kill,
            ..Default::default()
        });
        state.metrics.record_agent_loop_stopped();
        if auto_kill && !state.flight_recorder.kill_scope().halts_tools() {
            state.flight_recorder.set_kill_scope(KillScope::Tools);
            state.flight_recorder.record(FlightDraft {
                plane: "ops".into(),
                kind: FlightKind::KillSwitch,
                session: alert.session.clone(),
                route: "auto".into(),
                cache_status: "engaged".into(),
                detail: format!(
                    "kill switch auto-engaged (scope: tools) by chain rule '{}'",
                    alert.rule
                ),
                enforced: true,
                ..Default::default()
            });
        }
    }
}

pub fn governance_block_response(
    stream: bool,
    openai_style: bool,
    title: &str,
    detail: &str,
    circuit_open: bool,
) -> Response {
    let mut resp = if stream {
        let msg = if openai_style {
            format!(
                "data: {{\"choices\": [{{\"delta\": {{\"content\": \"\\n\\n[KOTRO {title}]: {detail}\"}},\"finish_reason\": \"stop\"}}]}}\n\ndata: [DONE]\n\n"
            )
        } else {
            format!(
                "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"\\n\\n[KOTRO {title}]: {detail}\"}}}}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
            )
        };
        let stream =
            futures_util::stream::once(async move { Ok::<_, std::io::Error>(Bytes::from(msg)) });
        let body = Body::from_stream(stream);
        let mut r = Response::new(body);
        *r.status_mut() = StatusCode::OK;
        r.headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
        r
    } else {
        problem_response(StatusCode::TOO_MANY_REQUESTS, title, detail)
    };
    if circuit_open {
        try_set_header(&mut resp, "x-kotro-circuit-open", "true");
    }
    try_set_header(&mut resp, "x-kotro-kill-switch", "tripped");
    resp
}

/// Early governance gate shared by OpenAI + Anthropic handlers.
/// Returns `Some(Response)` when the request must not proceed upstream.
pub fn check_early_governance(
    state: &AppState,
    unified: &UnifiedRequest,
    provider: &str,
    route: &str,
    openai_style: bool,
    session: &str,
    start: Instant,
) -> Option<Response> {
    let model = unified.model.as_str();
    let stream = unified.stream;
    let enforces = state.kill_switch_mode.enforces();

    if state.flight_recorder.kill_scope().halts_llm() {
        let detail = format!(
            "Kill switch engaged (scope: {}) — upstream LLM forwards halted.",
            state.flight_recorder.kill_scope().as_str()
        );
        record_flight(
            state,
            FlightKind::KillSwitch,
            provider,
            model,
            route,
            "blocked",
            Some(unified),
            session,
            0,
            start.elapsed(),
            0,
            &detail,
            enforces,
        );
        state.metrics.record_agent_loop_stopped();
        if enforces {
            return Some(governance_block_response(
                stream,
                openai_style,
                "KILL SWITCH",
                &detail,
                true,
            ));
        }
    }

    if let Err(limit) = state.request_rate.try_acquire(session) {
        let detail = format!(
            "Rate limit: session exceeded {limit} requests/minute (token bucket)."
        );
        record_flight(
            state,
            FlightKind::RateLimit,
            provider,
            model,
            route,
            "blocked",
            Some(unified),
            session,
            0,
            start.elapsed(),
            0,
            &detail,
            enforces,
        );
        state.metrics.record_agent_loop_stopped();
        if enforces {
            return Some(governance_block_response(
                stream,
                openai_style,
                "RATE LIMIT",
                &detail,
                true,
            ));
        }
    }

    if state.max_tool_rounds > 0 {
        let rounds = count_tool_rounds(&unified.messages);
        if rounds >= state.max_tool_rounds {
            let detail = format!(
                "Tool storm: {rounds} tool rounds in this conversation (max {}).",
                state.max_tool_rounds
            );
            record_flight(
                state,
                FlightKind::ToolStorm,
                provider,
                model,
                route,
                "blocked",
                Some(unified),
                session,
                0,
                start.elapsed(),
                0,
                &detail,
                enforces,
            );
            state.metrics.record_agent_loop_stopped();
            if enforces {
                return Some(governance_block_response(
                    stream,
                    openai_style,
                    "TOOL STORM",
                    &detail,
                    true,
                ));
            }
        }
    }

    None
}

#[allow(clippy::too_many_arguments)]
pub fn trip_circuit_breaker(
    state: &AppState,
    cache_key: &str,
    unified: &UnifiedRequest,
    provider: &str,
    route: &str,
    openai_style: bool,
    session: &str,
    start: Instant,
) -> Option<Response> {
    if state.circuit_breaker_threshold == 0 {
        return None;
    }
    let count = state.circuit_breaker.get(cache_key).unwrap_or(0) + 1;
    state.circuit_breaker.insert(cache_key.to_string(), count);
    if count < state.circuit_breaker_threshold {
        return None;
    }

    let enforces = state.kill_switch_mode.enforces();
    let detail = format!(
        "Identical prompt-state seen {count} times within {}s. Halting to prevent credit drain.",
        state.circuit_breaker_window_secs
    );
    tracing::warn!(key = %cache_key, count = count, "circuit breaker tripped");
    record_flight(
        state,
        FlightKind::CircuitOpen,
        provider,
        &unified.model,
        route,
        "blocked",
        Some(unified),
        session,
        0,
        start.elapsed(),
        0,
        &detail,
        enforces,
    );
    state.metrics.record_agent_loop_stopped();

    if !enforces {
        record_flight(
            state,
            FlightKind::Observe,
            provider,
            &unified.model,
            route,
            "miss",
            Some(unified),
            session,
            0,
            start.elapsed(),
            0,
            "circuit breaker observe-only — request continues",
            false,
        );
        return None;
    }

    Some(governance_block_response(
        unified.stream,
        openai_style,
        "CIRCUIT BREAKER",
        &detail,
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_limit_is_unlimited() {
        let limiter = SessionRateLimiter::new(0);
        for _ in 0..1000 {
            assert!(limiter.try_acquire("s").is_ok());
        }
    }

    #[test]
    fn sessions_are_isolated() {
        let limiter = SessionRateLimiter::new(3);
        // Session A exhausts its bucket.
        for _ in 0..3 {
            assert!(limiter.try_acquire("a").is_ok());
        }
        assert!(limiter.try_acquire("a").is_err());
        // Session B still has a full bucket.
        assert!(limiter.try_acquire("b").is_ok());
    }
}
