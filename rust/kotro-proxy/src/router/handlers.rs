//! Axum route handlers.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{error, info};

use crate::budget::BudgetTracker;
use crate::cache::{generate_cache_key, Entry};
use crate::compressor::Scope;
use crate::guardrail::{InjectionFinding, RedactionMap};
use crate::router::classifier::{classify_complexity, PromptComplexity};
// redact_chat_request / redact_messages_request are defined in crate::guardrail but
// not yet called here — redaction is currently done via apply_unified_middleware's
// passthrough stub. Import only what is used.
// TODO: wire per-format redaction once redact_unified_request is implemented.
use crate::models::{anthropic::MessagesRequest, openai::ChatCompletionRequest, unified::UnifiedRequest};
use crate::proxy::pipeline::{create_processing_pipeline, PipelineOptions, StreamFormat};
use crate::proxy::replay::create_cached_replay_stream;
use crate::router::AppState;
use crate::router::bridge_auth::{self, UpstreamAuthStyle};
use crate::router::upstream;

const UPSTREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Extension / CLI smoke-test model — never forwarded upstream (no API key required).
pub const LOCAL_VERIFY_MODEL: &str = "kotro-local-verify";

const LOCAL_VERIFY_SSE: &[u8] = b"data: {\"id\":\"kotro-verify\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"kotro local verify ok\"}}]}\n\ndata: [DONE]\n\n";

#[derive(Serialize)]
struct ProblemDetails {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
}

fn problem_response(status: StatusCode, title: &str, detail: &str) -> Response {
    let pd = ProblemDetails {
        problem_type: "https://docs.kotrolabs.com/errors".to_string(),
        title: title.to_string(),
        status: status.as_u16(),
        detail: detail.to_string(),
    };
    let mut response = Json(pd).into_response();
    *response.status_mut() = status;
    response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/problem+json"));
    response
}

/// Run the WASM plugin chain against an incoming request body.
///
/// Returns:
/// - `Ok(None)`          — plugins passed, body unchanged
/// - `Ok(Some(body))`   — plugins passed, body replaced with new JSON string
/// - `Err(Response)`    — a plugin blocked the request; return the response immediately
///
/// Warnings from plugin errors are logged but never surface as hard failures so
/// that a broken plugin cannot take down the proxy's primary request path.
fn run_wasm_plugins(
    plugin_manager: &crate::plugins::wasm::PluginManager,
    uri: &str,
    headers: &HeaderMap,
    body: &str,
) -> Result<Option<String>, Response> {
    if plugin_manager.is_empty() {
        return Ok(None);
    }

    let mut wasm_headers = std::collections::HashMap::new();
    for (k, v) in headers.iter() {
        if let Ok(val) = v.to_str() {
            wasm_headers.insert(k.as_str().to_string(), val.to_string());
        }
    }

    let wasm_req = crate::plugins::wasm::WasmRequest {
        uri: uri.to_string(),
        method: "POST".to_string(),
        headers: wasm_headers,
        body: body.to_string(),
    };

    match plugin_manager.on_request(wasm_req) {
        Ok(wasm_res) => {
            if wasm_res.block.unwrap_or(false) {
                return Err(problem_response(
                    StatusCode::FORBIDDEN,
                    "Blocked by Plugin",
                    "A WASM plugin blocked this request.",
                ));
            }
            Ok(wasm_res.body)
        }
        Err(e) => {
            tracing::warn!("Failed to execute WASM plugins: {}", e);
            Ok(None)
        }
    }
}


/// Attach a single header to a [`Response`].
///
/// Silently ignores invalid names or values rather than panicking, matching the
/// philosophy that observability headers must never break the primary request path.
fn try_set_header(resp: &mut Response, name: &str, value: &str) {
    if let (Ok(n), Ok(v)) = (
        axum::http::HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_str(value),
    ) {
        resp.headers_mut().insert(n, v);
    }
}

/// Attach injection and budget headers to a response in one call.
fn attach_guardrail_headers(
    resp: &mut Response,
    finding: Option<&InjectionFinding>,
    tokens_used: u64,
    budget: &BudgetTracker,
    scope_key: &str,
) {
    try_set_header(resp, "x-kotro-tokens-used", &tokens_used.to_string());
    if budget.limit_tokens > 0 {
        try_set_header(
            resp,
            "x-kotro-budget-remaining",
            &budget.remaining(scope_key).to_string(),
        );
    }
    if let Some(f) = finding {
        try_set_header(resp, "x-kotro-injection-warning", f.pattern_name);
    }
}

/// Extract `role: "tool"` messages from a unified message list and populate the
/// tool result cache. Also detects write operations and invalidates related read
/// entries so stale file contents are never served after a write.
///
/// Returns the number of results stored (new entries only; overwrites count too).
fn populate_tool_cache(
    messages: &[crate::models::unified::UnifiedMessage],
    tool_cache: &crate::cache::tool::ToolCache,
    scope_key: &str,
) -> usize {
    use crate::cache::tool::ToolCategory;

    if !tool_cache.enabled {
        return 0;
    }

    let mut stored = 0usize;

    // Build a map from tool_call_id → (function_name, args_json) by scanning
    // assistant messages for `tool_calls` arrays.
    let mut call_meta: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();
    for msg in messages {
        if msg.role == "assistant" {
            if let Some(tool_calls) = &msg.tool_calls {
                if let Some(arr) = tool_calls.as_array() {
                    for tc in arr {
                        let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let args = tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .map(|v| v.as_str().unwrap_or(&v.to_string()).to_string())
                            .unwrap_or_default();
                        if !id.is_empty() {
                            call_meta.insert(id, (name, args));
                        }
                    }
                }
            }
        }
    }

    // Now scan tool result messages and cache their content.
    for msg in messages {
        if msg.role == "tool" {
            let call_id = msg.tool_call_id.as_deref().unwrap_or("");
            let content = crate::models::unified::content_text(&msg.content);
            if content.is_empty() {
                continue;
            }
            if let Some((name, args)) = call_meta.get(call_id) {
                // Detect write operations and invalidate stale read entries.
                if ToolCategory::is_write(name) {
                    // Best-effort: extract path from args to scope the invalidation.
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
                        if let Some(path) = v.get("path").and_then(|p| p.as_str()) {
                            tool_cache.invalidate_by_path(path);
                        }
                    }
                } else {
                    tool_cache.put(scope_key, name, args, &content);
                    stored += 1;
                }
            } else {
                // No matching call_id — cache under a fallback key derived from content.
                tool_cache.put(scope_key, "unknown_tool", call_id, &content);
                stored += 1;
            }
        }
    }

    stored
}

const SSE_HEADERS: [(&str, &str); 4] = [
    ("content-type", "text/event-stream"),
    ("cache-control", "no-cache"),
    ("connection", "keep-alive"),
    ("x-accel-buffering", "no"),
];

pub async fn handle_healthz() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(CONTENT_TYPE.as_str(), "application/json")],
        r#"{"status":"ok","service":"kotro-proxy"}"#,
    )
}

pub async fn handle_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(CONTENT_TYPE.as_str(), "text/plain; version=0.0.4; charset=utf-8")],
        state.metrics.gather_to_string(),
    )
}

pub async fn handle_dashboard(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(CONTENT_TYPE.as_str(), "text/html; charset=utf-8")],
        crate::dashboard_assets::PAGE_HTML,
    )
}

pub async fn handle_api_dashboard(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let snap = state.metrics.snapshot();
    (
        StatusCode::OK,
        [(CONTENT_TYPE.as_str(), "application/json")],
        serde_json::to_string_pretty(&snap).unwrap_or_default(),
    )
}

pub async fn handle_icon(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [
            (CONTENT_TYPE.as_str(), "image/png"),
            ("cache-control", "public, max-age=86400"),
        ],
        crate::dashboard_assets::ICON_PNG,
    )
}

struct StreamGuard {
    metrics: crate::metrics::MetricsRegistry,
    provider: &'static str,
    model: String,
    route: &'static str,
    stream_flag: bool,
    cache_status: &'static str,
    start_time: Instant,
    recorded: bool,
}

impl StreamGuard {
    fn record(&mut self) {
        if !self.recorded {
            let elapsed = self.start_time.elapsed();
            self.metrics.record_request(
                self.provider,
                &self.model,
                self.route,
                self.stream_flag,
                self.cache_status,
                elapsed,
            );
            self.recorded = true;
        }
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.record();
    }
}

fn instrument_stream<S>(
    stream: S,
    metrics: crate::metrics::MetricsRegistry,
    provider: &'static str,
    model: String,
    route: &'static str,
    stream_flag: bool,
    cache_status: &'static str,
    start_time: Instant,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static
where
    S: futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let mut guard = StreamGuard {
        metrics,
        provider,
        model,
        route,
        stream_flag,
        cache_status,
        start_time,
        recorded: false,
    };

    async_stream::try_stream! {
        tokio::pin!(stream);

        while let Some(item) = stream.next().await {
            let bytes = item?;
            yield bytes;
        }

        guard.record();
    }
}

fn create_primed_miss_stream(
    state: Arc<AppState>,
    headers: HeaderMap,
    path: String,
    payload_bytes: Vec<u8>,
    pipeline_opts: PipelineOptions,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    async_stream::try_stream! {
        yield crate::proxy::bootstrap::bootstrap_bytes();

        let base_url = get_upstream_url(&state, &pipeline_opts.model);
        let start_upstream = Instant::now();
        let auth_style = match pipeline_opts.format {
            StreamFormat::OpenAI => UpstreamAuthStyle::Bearer,
            StreamFormat::Anthropic => UpstreamAuthStyle::AnthropicXApiKey,
        };
        let upstream_response = match post_with_failover(
            &state,
            &headers,
            base_url,
            &path,
            payload_bytes.clone(),
            auth_style,
        )
        .await
        {
            Ok(resp) => resp,
            Err(kind) => {
                let provider_str = match pipeline_opts.format {
                    StreamFormat::OpenAI => "openai",
                    StreamFormat::Anthropic => "anthropic",
                };
                state.metrics.record_error(provider_str, kind);
                let err_msg = match kind {
                    "timeout" => "data: {\"error\": \"Gateway timeout: Upstream provider did not respond in time\"}\n\n".to_string(),
                    "misconfigured" => "data: {\"error\": \"KOTRO_BRIDGE_TOKEN set but KOTRO_UPSTREAM_API_KEY missing\"}\n\n".to_string(),
                    _ => "data: {\"error\": \"Upstream network failure\"}\n\n".to_string(),
                };
                yield Bytes::from(err_msg);
                return;
            }
        };

        let status = upstream_response.status();
        let provider_str = match pipeline_opts.format {
            StreamFormat::OpenAI => "openai",
            StreamFormat::Anthropic => "anthropic",
        };
        state.metrics.record_upstream(provider_str, status.as_u16(), start_upstream.elapsed());

        if !status.is_success() {
            let err_bytes = upstream_response.bytes().await.unwrap_or_default();
            state.metrics.record_error(provider_str, "upstream");
            yield err_bytes;
            return;
        }

        let upstream_byte_stream = upstream_response.bytes_stream();
        let outbound = create_processing_pipeline(
            upstream_byte_stream,
            state.store.clone(),
            pipeline_opts,
        );

        tokio::pin!(outbound);
        let mut first = true;
        while let Some(chunk_result) = outbound.next().await {
            let chunk = chunk_result?;
            if first {
                first = false;
                if chunk.starts_with(b": kotrolabs bootstrap") {
                    continue;
                }
            }
            yield chunk;
        }
    }
}

async fn post_with_failover(
    state: &AppState,
    headers: &HeaderMap,
    primary_base: &str,
    path: &str,
    body: Vec<u8>,
    auth_style: UpstreamAuthStyle,
) -> Result<reqwest::Response, &'static str> {
    let headers = match bridge_auth::prepare_upstream_headers(
        state.bridge_token.as_deref(),
        state.upstream_api_key.as_deref(),
        headers,
        auth_style,
    ) {
        Ok(h) => h,
        Err(bridge_auth::BridgeAuthError::UpstreamKeyRequired) => return Err("misconfigured"),
        Err(_) => return Err("upstream"),
    };

    let primary = format!("{}{}", primary_base, path);
    let primary_req =
        with_forwarded_headers(state.http_client.post(primary).body(body.clone()), &headers);

    match tokio::time::timeout(UPSTREAM_TIMEOUT, primary_req.send()).await {
        Ok(Ok(resp)) if !upstream::should_failover(resp.status(), false) => return Ok(resp),
        Ok(Ok(resp)) => {
            let _ = resp.bytes().await;
        }
        Ok(Err(_)) if state.fallback_url.is_none() => return Err("upstream"),
        Ok(Err(_)) => {}
        Err(_) if state.fallback_url.is_none() => return Err("timeout"),
        Err(_) => {}
    }

    let Some(fallback_base) = state.fallback_url.as_deref() else {
        return Err("upstream");
    };

    let fallback = format!("{}{}", fallback_base, path);
    let fallback_req =
        with_forwarded_headers(state.http_client.post(fallback).body(body), &headers);

    match tokio::time::timeout(UPSTREAM_TIMEOUT, fallback_req.send()).await {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(_)) => Err("upstream"),
        Err(_) => Err("timeout"),
    }
}

/// Runs the CPU-bound semantic-cache embedding step off the async runtime's
/// worker threads.
///
/// `SemanticEncoder::embed()` does real BERT inference (~25-30ms measured
/// flat across prompt sizes, see `examples/bench_embedding.rs`) and used to
/// run synchronously inline in the two call sites below, blocking a tokio
/// worker thread for that duration on every cache-eligible request --
/// competing directly with the async I/O work (upstream forwarding, SSE
/// streaming) other concurrent requests need that same thread pool for.
/// `spawn_blocking` moves it to tokio's dedicated blocking thread pool
/// instead, matching the docs/roadmap/next-steps.md P2 follow-up.
///
/// A panic inside the embedding call surfaces as a `JoinError` here rather
/// than unwinding into the request handler; treated the same as any other
/// embedding failure (see `SemanticEncoder::embed`'s own error handling) --
/// logged and folded into "no vector-cache hit for this request," not a
/// fatal error. The exact-match cache path above this call is unaffected.
async fn embed_off_thread(
    encoder: Arc<crate::cache::vector::SemanticEncoder>,
    text: String,
) -> Option<Vec<f32>> {
    match tokio::task::spawn_blocking(move || encoder.embed(&text)).await {
        Ok(result) => result,
        Err(join_err) => {
            tracing::warn!(
                error = %join_err,
                "semantic embedding task panicked; treating as no vector-cache hit"
            );
            None
        }
    }
}

pub async fn handle_chat_completions(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(err) =
        bridge_auth::require_bridge_token(state.bridge_token.as_deref(), &headers)
    {
        return err.into_response();
    }

    let start_time = Instant::now();
    let mut req: ChatCompletionRequest = match serde_json::from_value(payload) {
        Ok(req) => req,
        Err(err) => {
            state.metrics.record_error("openai", "parse");
            return problem_response(StatusCode::BAD_REQUEST, "Invalid Request", &format!("invalid json: {err}"));
        }
    };

    crate::optimizer::enforce_cache_matrix(&mut req);

    let mut body_str = serde_json::to_string(&req).unwrap_or_default();

    match run_wasm_plugins(&state.plugin_manager, "/v1/chat/completions", &headers, &body_str) {
        Ok(Some(new_body)) => {
            if let Ok(new_req) = serde_json::from_str(&new_body) {
                req = new_req;
                body_str = new_body;
            } else {
                tracing::warn!("WASM plugin returned invalid modified body, ignoring.");
            }
        }
        Ok(None) => {}
        Err(blocked) => return blocked,
    }

    state.metrics.record_request_body("openai", body_str.len());

    let scope = state.scope.from_request(&headers, peer.ip());
    let (_, latest_user) = req.extract_prompt_state();

    // ── Complexity-based model routing ───────────────────────────────────────
    // Route cheap/trivial requests to cheaper models before building unified_req
    // so the final model name propagates through the entire pipeline.
    // Never rewrite the keyless smoke-test model.
    if req.model != LOCAL_VERIFY_MODEL {
        let has_tool_calls = req.messages.iter().any(|m| m.tool_calls.is_some());
        match classify_complexity(&latest_user, req.messages.len(), has_tool_calls) {
            PromptComplexity::Nano => {
                if state.local_upstream_url.is_some() {
                    tracing::info!(model = %state.moe_default_model, "model router: Nano → local model");
                    req.model = state.moe_default_model.clone();
                }
            }
            PromptComplexity::Micro => {
                if let Some(ref cheap) = state.cheap_model {
                    tracing::info!(model = %cheap, "model router: Micro → cheap model");
                    req.model = cheap.clone();
                }
            }
            PromptComplexity::Complex => {
                tracing::info!(tier = "complex", model = %req.model, "model router: Complex tier — keeping configured model");
            }
            PromptComplexity::Standard => {}
        }
    }

    let unified_req: UnifiedRequest = req.clone().try_into().unwrap_or_else(|_| {
        // Fallback or handle error
        UnifiedRequest {
            model: req.model.clone(),
            system_prompt: "".into(),
            messages: vec![],
            stream: req.stream,
            max_tokens: None,
        }
    });

    // ── Tool result cache (populate) ─────────────────────────────────────────
    // Extract role:"tool" messages and store results before the cache lookup so
    // subsequent identical tool calls can be served without re-executing the tool.
    {
        let scope_key_early = scope.key();
        let stored = populate_tool_cache(&unified_req.messages, &state.tool_cache, &scope_key_early);
        if stored > 0 {
            tracing::debug!(count = stored, scope = %scope_key_early, "tool cache: stored {} result(s)", stored);
        }
    }

    // ── Injection scan ────────────────────────────────────────────────────────
    // Run before the cache lookup so we never serve a cached response to a
    // poisoned request, and never cache the downstream result of one.
    let injection_finding: Option<InjectionFinding> = if state.enable_injection_scan {
        crate::guardrail::scan_messages(&unified_req.messages)
    } else {
        None
    };
    if let Some(ref finding) = injection_finding {
        state.metrics.record_injection_detected();
        tracing::warn!(
            pattern = finding.pattern_name,
            role = %finding.role,
            snippet = %finding.matched_snippet,
            "kotro guardrail: MCP prompt injection detected"
        );
        if state.injection_block_on_detection {
            state.metrics.record_injection_blocked();
            // Count blocked requests in traffic totals / recent table (otherwise
            // dashboard shows injections with requests_total=0).
            state.metrics.record_request(
                "openai",
                &req.model,
                "/v1/chat/completions",
                req.stream,
                "blocked",
                start_time.elapsed(),
            );
            return problem_response(
                StatusCode::BAD_REQUEST,
                "Prompt Injection Detected",
                &format!(
                    "Pattern '{}' detected in {} message. Request blocked by Kotro guardrail.",
                    finding.pattern_name, finding.role
                ),
            );
        }
    }

    // ── Agent tool-call loop detection ───────────────────────────────────────
    // Catches the case where an agent calls the same tool with identical args
    // across multiple turns. The request-level circuit breaker (cache-key based)
    // doesn't catch this because the surrounding context keeps changing.
    if let Some(ref lf) = crate::guardrail::detect_tool_call_loops(
        &unified_req.messages, state.tool_loop_threshold,
    ) {
        state.metrics.record_agent_loop_stopped();
        tracing::warn!(
            function = %lf.function_name,
            count = lf.call_count,
            threshold = state.tool_loop_threshold,
            "kotro guardrail: tool-call loop detected"
        );
        if unified_req.stream {
            let msg = format!(
                "data: {{\"choices\": [{{\"delta\": {{\"content\": \"\\n\\n🔁 [KOTRO LOOP DETECTED]: Tool \\\"{}\\\" was called {} times with identical arguments. Breaking agent loop.\"}}}}, {{\"finish_reason\": \"stop\"}}]}}\n\ndata: [DONE]\n\n",
                lf.function_name, lf.call_count
            );
            let stream = futures_util::stream::once(async move { Ok::<_, io::Error>(Bytes::from(msg)) });
            return sse_stream_response(stream, None);
        } else {
            return problem_response(
                StatusCode::TOO_MANY_REQUESTS,
                "Agent Loop Detected",
                &format!(
                    "Tool '{}' was called {} times with identical arguments. Breaking loop.",
                    lf.function_name, lf.call_count
                ),
            );
        }
    }

    // ── Token estimation (used for budget and response headers) ───────────────
    let scope_key = scope.key();
    let estimated_input_tokens: u64 = {
        let mut n = BudgetTracker::estimate_tokens(&unified_req.system_prompt);
        for msg in &unified_req.messages {
            n = n.saturating_add(BudgetTracker::estimate_tokens(
                &crate::models::openai::content_text(&msg.content),
            ));
        }
        n
    };

    let (processed, cache_source, redaction_map) =
        apply_unified_middleware(&state, unified_req, &scope);
    let cache_key = unified_cache_key(&state, &scope, &cache_source, "openai");

    if !cache_key.is_empty() {
        if let Ok(Some(entry)) = state.store.get(&cache_key) {
            info!(key = %cache_key, format = "openai", "cache hit");
            state.metrics.record_cache_hit("openai", entry.raw_sse.len());
            let stream = create_cached_replay_stream(
                entry.raw_sse,
                redaction_map.clone(),
                state.cache_hit_delay,
                StreamFormat::OpenAI,
                state.metrics.clone(),
            );
            let instrumented = instrument_stream(
                stream,
                state.metrics.clone(),
                "openai",
                processed.model.clone(),
                "/v1/chat/completions",
                true,
                "hit",
                start_time,
            );
            let mut resp = sse_stream_response(instrumented, Some("HIT"));
            // Cache hits don't count toward budget (zero upstream cost), but
            // surface the current usage + injection warning if present.
            attach_guardrail_headers(
                &mut resp, injection_finding.as_ref(),
                state.budget.current(&scope_key), &state.budget, &scope_key,
            );
            return resp;
        }
        info!(key = %cache_key, format = "openai", "cache miss");

        // Keyless smoke test: store a fixture SSE and return MISS (second identical
        // request hits the exact store). Never calls upstream.
        if processed.model == LOCAL_VERIFY_MODEL {
            return serve_local_verify_miss(
                &state,
                cache_key,
                processed.model.clone(),
                redaction_map,
                injection_finding.as_ref(),
                &scope_key,
                start_time,
            );
        }

        if let Some(user_emb) =
            embed_off_thread(state.vector_encoder.clone(), latest_user.clone()).await
        {
            let context_key = format!("{}:{}:openai", scope.key(), processed.model);
            if let Some(similar_key) = state.vector_index.find_closest(&context_key, &user_emb, 0.94) {
                if let Ok(Some(entry)) = state.store.get(&similar_key) {
                    tracing::info!(key = %similar_key, "Semantic cache HIT via vector index for similar intent!");
                    state.metrics.record_cache_hit("openai", entry.raw_sse.len());
                    let stream = create_cached_replay_stream(
                        entry.raw_sse,
                        redaction_map.clone(),
                        state.cache_hit_delay,
                        StreamFormat::OpenAI,
                        state.metrics.clone(),
                    );
                    let instrumented = instrument_stream(
                        stream,
                        state.metrics.clone(),
                        "openai",
                        processed.model.clone(),
                        "/v1/chat/completions",
                        true,
                        "semantic",
                        start_time,
                    );
                    let mut resp = sse_stream_response(instrumented, Some("SEMANTIC"));
                    attach_guardrail_headers(
                        &mut resp, injection_finding.as_ref(),
                        state.budget.current(&scope_key), &state.budget, &scope_key,
                    );
                    return resp;
                }
            }
            state.vector_index.insert(context_key, cache_key.clone(), latest_user.clone(), user_emb);
        }

        state.metrics.record_cache_miss("openai");

        // ── Budget enforcement (cache misses only) ────────────────────────────
        if state.budget.is_exceeded(&scope_key) {
            state.metrics.record_budget_hit();
            tracing::warn!(
                scope = %scope_key,
                limit = state.budget.limit_tokens,
                used = state.budget.current(&scope_key),
                "kotro guardrail: session token budget exceeded"
            );
            if state.budget.block_on_exceeded {
                return problem_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Session Budget Exceeded",
                    &format!(
                        "Session token budget of {} tokens exceeded. Resets after idle period.",
                        state.budget.limit_tokens
                    ),
                );
            }
        }

        let count = state.circuit_breaker.get(&cache_key).unwrap_or(0) + 1;
        state.circuit_breaker.insert(cache_key.clone(), count);
        if count >= 4 {
            state.metrics.record_agent_loop_stopped();
            tracing::warn!(key = %cache_key, count = count, "circuit breaker tripped");
            if processed.stream {
                let err_msg = "data: {\"choices\": [{\"delta\": {\"content\": \"\\n\\n🚨 [KOTRO CIRCUIT BREAKER TRIPPED]: Infinite error loop detected. Halting execution to prevent API credit drain. Please ask the human operator for guidance.\"}}]}\n\ndata: [DONE]\n\n";
                let stream = futures_util::stream::once(async move { Ok::<_, io::Error>(Bytes::from(err_msg)) });
                return sse_stream_response(stream, None);
            } else {
                return problem_response(StatusCode::TOO_MANY_REQUESTS, "Circuit Breaker Tripped", "Infinite error loop detected. Halting execution to prevent API credit drain.");
            }
        }
    }

    let mut final_req: ChatCompletionRequest = processed.clone().into();

    // ── Reasoning model budget controller (OpenAI path) ───────────────────────
    if state.reasoning_block
        && crate::optimizer::reasoning::is_openai_reasoning_model(&final_req.model)
    {
        return problem_response(
            StatusCode::FORBIDDEN,
            "Reasoning Model Blocked",
            "Requests to reasoning models are blocked by policy (KOTRO_REASONING_BLOCK=true). \
             Use a non-reasoning model or disable KOTRO_REASONING_BLOCK.",
        );
    }
    if state.max_thinking_tokens > 0 {
        use crate::optimizer::reasoning::{apply_openai_reasoning_budget, ReasoningOutcome};
        if let ReasoningOutcome::Capped { cap } =
            apply_openai_reasoning_budget(&mut final_req, state.max_thinking_tokens)
        {
            tracing::info!(
                model = %final_req.model,
                cap = cap,
                "reasoning budget: capped max_completion_tokens"
            );
        }
    }

    let mut resp = forward_provider(
        &state,
        &headers,
        ForwardOptions {
            path: "/v1/chat/completions",
            payload_bytes: serde_json::to_vec(&final_req).unwrap_or_default(),
            request_streaming: processed.stream,
            cache_key,
            model: processed.model.clone(),
            format: StreamFormat::OpenAI,
            redaction_map,
            start_time,
            provider: "openai",
        }
    )
    .await;

    // Record budget usage and attach guardrail headers.
    let tokens_used = state.budget.record(&scope_key, estimated_input_tokens);
    attach_guardrail_headers(&mut resp, injection_finding.as_ref(), tokens_used, &state.budget, &scope_key);
    resp
}

pub async fn handle_messages(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(err) =
        bridge_auth::require_bridge_token(state.bridge_token.as_deref(), &headers)
    {
        return err.into_response();
    }

    let start_time = Instant::now();
    let mut req: MessagesRequest = match serde_json::from_value(payload) {
        Ok(req) => req,
        Err(err) => {
            state.metrics.record_error("anthropic", "parse");
            return problem_response(StatusCode::BAD_REQUEST, "Invalid Request", &format!("invalid json: {err}"));
        }
    };

    let mut body_str = serde_json::to_string(&req).unwrap_or_default();

    match run_wasm_plugins(&state.plugin_manager, "/v1/messages", &headers, &body_str) {
        Ok(Some(new_body)) => {
            if let Ok(new_req) = serde_json::from_str(&new_body) {
                req = new_req;
                body_str = new_body;
            } else {
                tracing::warn!("WASM plugin returned invalid modified body, ignoring.");
            }
        }
        Ok(None) => {}
        Err(blocked) => return blocked,
    }

    state.metrics.record_request_body("anthropic", body_str.len());

    let scope = state.scope.from_request(&headers, peer.ip());
    let (_, latest_user) = req.extract_prompt_state();

    let mut unified_req: UnifiedRequest = req.clone().try_into().unwrap_or_else(|_| {
        UnifiedRequest {
            model: req.model.clone(),
            system_prompt: "".into(),
            messages: vec![],
            stream: req.stream,
            max_tokens: Some(req.max_tokens),
        }
    });

    // ── Complexity-based model routing ────────────────────────────────────────
    {
        let has_tool_calls = unified_req.messages.iter().any(|m| m.tool_calls.is_some());
        match classify_complexity(&latest_user, unified_req.messages.len(), has_tool_calls) {
            PromptComplexity::Nano => {
                if state.local_upstream_url.is_some() {
                    tracing::info!(model = %state.moe_default_model, "model router: Nano → local model");
                    unified_req.model = state.moe_default_model.clone();
                }
            }
            PromptComplexity::Micro => {
                if let Some(ref cheap) = state.cheap_model {
                    tracing::info!(model = %cheap, "model router: Micro → cheap model");
                    unified_req.model = cheap.clone();
                }
            }
            PromptComplexity::Complex => {
                tracing::info!(tier = "complex", model = %unified_req.model, "model router: Complex tier — keeping configured model");
            }
            PromptComplexity::Standard => {}
        }
    }

    // ── Tool result cache (populate) ─────────────────────────────────────────
    {
        let scope_key_early = scope.key();
        let stored = populate_tool_cache(&unified_req.messages, &state.tool_cache, &scope_key_early);
        if stored > 0 {
            tracing::debug!(count = stored, scope = %scope_key_early, "tool cache: stored {} result(s)", stored);
        }
    }

    // ── Injection scan ────────────────────────────────────────────────────────
    let injection_finding: Option<InjectionFinding> = if state.enable_injection_scan {
        crate::guardrail::scan_messages(&unified_req.messages)
    } else {
        None
    };
    if let Some(ref finding) = injection_finding {
        state.metrics.record_injection_detected();
        tracing::warn!(
            pattern = finding.pattern_name,
            role = %finding.role,
            snippet = %finding.matched_snippet,
            "kotro guardrail: MCP prompt injection detected"
        );
        if state.injection_block_on_detection {
            state.metrics.record_injection_blocked();
            state.metrics.record_request(
                "anthropic",
                &req.model,
                "/v1/messages",
                req.stream,
                "blocked",
                start_time.elapsed(),
            );
            return problem_response(
                StatusCode::BAD_REQUEST,
                "Prompt Injection Detected",
                &format!(
                    "Pattern '{}' detected in {} message. Request blocked by Kotro guardrail.",
                    finding.pattern_name, finding.role
                ),
            );
        }
    }

    // ── Agent tool-call loop detection ────────────────────────────────────────
    if let Some(ref lf) = crate::guardrail::detect_tool_call_loops(
        &unified_req.messages, state.tool_loop_threshold,
    ) {
        state.metrics.record_agent_loop_stopped();
        tracing::warn!(
            function = %lf.function_name,
            count = lf.call_count,
            threshold = state.tool_loop_threshold,
            "kotro guardrail: tool-call loop detected"
        );
        if unified_req.stream {
            let msg = format!(
                "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"\\n\\n🔁 [KOTRO LOOP DETECTED]: Tool \\\"{}\\\" was called {} times with identical arguments. Breaking agent loop.\"}}}}\n\n",
                lf.function_name, lf.call_count
            );
            let stream = futures_util::stream::once(async move { Ok::<_, io::Error>(Bytes::from(msg)) });
            return sse_stream_response(stream, None);
        } else {
            return problem_response(
                StatusCode::TOO_MANY_REQUESTS,
                "Agent Loop Detected",
                &format!(
                    "Tool '{}' was called {} times with identical arguments. Breaking loop.",
                    lf.function_name, lf.call_count
                ),
            );
        }
    }

    // ── Token estimation ──────────────────────────────────────────────────────
    let scope_key = scope.key();
    let estimated_input_tokens: u64 = {
        let mut n = BudgetTracker::estimate_tokens(&unified_req.system_prompt);
        for msg in &unified_req.messages {
            n = n.saturating_add(BudgetTracker::estimate_tokens(
                &crate::models::openai::content_text(&msg.content),
            ));
        }
        n
    };

    let (processed, cache_source, redaction_map) =
        apply_unified_middleware(&state, unified_req, &scope);
    let cache_key = unified_cache_key(&state, &scope, &cache_source, "anthropic");

    if !cache_key.is_empty() {
        if let Ok(Some(entry)) = state.store.get(&cache_key) {
            info!(key = %cache_key, format = "anthropic", "cache hit");
            state.metrics.record_cache_hit("anthropic", entry.raw_sse.len());
            let stream = create_cached_replay_stream(
                entry.raw_sse,
                redaction_map,
                state.cache_hit_delay,
                StreamFormat::Anthropic,
                state.metrics.clone(),
            );
            let instrumented = instrument_stream(
                stream,
                state.metrics.clone(),
                "anthropic",
                processed.model.clone(),
                "/v1/messages",
                true,
                "hit",
                start_time,
            );
            let mut resp = sse_stream_response(instrumented, Some("HIT"));
            attach_guardrail_headers(
                &mut resp, injection_finding.as_ref(),
                state.budget.current(&scope_key), &state.budget, &scope_key,
            );
            return resp;
        }
        info!(key = %cache_key, format = "anthropic", "cache miss");

        if let Some(user_emb) =
            embed_off_thread(state.vector_encoder.clone(), latest_user.clone()).await
        {
            let context_key = format!("{}:{}:anthropic", scope.key(), processed.model);
            if let Some(similar_key) = state.vector_index.find_closest(&context_key, &user_emb, 0.94) {
                if let Ok(Some(entry)) = state.store.get(&similar_key) {
                    tracing::info!(key = %similar_key, "Semantic cache HIT via vector index for similar intent!");
                    state.metrics.record_cache_hit("anthropic", entry.raw_sse.len());
                    let stream = create_cached_replay_stream(
                        entry.raw_sse,
                        redaction_map.clone(),
                        state.cache_hit_delay,
                        StreamFormat::Anthropic,
                        state.metrics.clone(),
                    );
                    let instrumented = instrument_stream(
                        stream,
                        state.metrics.clone(),
                        "anthropic",
                        processed.model.clone(),
                        "/v1/messages",
                        true,
                        "semantic",
                        start_time,
                    );
                    let mut resp = sse_stream_response(instrumented, Some("SEMANTIC"));
                    attach_guardrail_headers(
                        &mut resp, injection_finding.as_ref(),
                        state.budget.current(&scope_key), &state.budget, &scope_key,
                    );
                    return resp;
                }
            }
            state.vector_index.insert(context_key, cache_key.clone(), latest_user.clone(), user_emb);
        }

        state.metrics.record_cache_miss("anthropic");

        // ── Budget enforcement (cache misses only) ────────────────────────────
        if state.budget.is_exceeded(&scope_key) {
            state.metrics.record_budget_hit();
            tracing::warn!(
                scope = %scope_key,
                limit = state.budget.limit_tokens,
                used = state.budget.current(&scope_key),
                "kotro guardrail: session token budget exceeded"
            );
            if state.budget.block_on_exceeded {
                return problem_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Session Budget Exceeded",
                    &format!(
                        "Session token budget of {} tokens exceeded. Resets after idle period.",
                        state.budget.limit_tokens
                    ),
                );
            }
        }

        let count = state.circuit_breaker.get(&cache_key).unwrap_or(0) + 1;
        state.circuit_breaker.insert(cache_key.clone(), count);
        if count >= 4 {
            state.metrics.record_agent_loop_stopped();
            tracing::warn!(key = %cache_key, count = count, "circuit breaker tripped");
            if processed.stream {
                let err_msg = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"\\n\\n🚨 [KOTRO CIRCUIT BREAKER TRIPPED]: Infinite error loop detected. Halting execution to prevent API credit drain. Please ask the human operator for guidance.\"}}\n\n";
                let stream = futures_util::stream::once(async move { Ok::<_, io::Error>(Bytes::from(err_msg)) });
                return sse_stream_response(stream, None);
            } else {
                return problem_response(StatusCode::TOO_MANY_REQUESTS, "Circuit Breaker Tripped", "Infinite error loop detected. Halting execution to prevent API credit drain.");
            }
        }
    }

    let mut final_req: MessagesRequest = processed.clone().into();

    // ── Reasoning model budget controller (Anthropic path) ────────────────────
    if state.reasoning_block
        && crate::optimizer::reasoning::is_anthropic_reasoning_model(&final_req.model)
    {
        return problem_response(
            StatusCode::FORBIDDEN,
            "Reasoning Model Blocked",
            "Requests to reasoning models are blocked by policy (KOTRO_REASONING_BLOCK=true). \
             Use a non-reasoning model or disable KOTRO_REASONING_BLOCK.",
        );
    }
    if state.max_thinking_tokens > 0 {
        use crate::optimizer::reasoning::{apply_anthropic_reasoning_budget, ReasoningOutcome};
        if let ReasoningOutcome::Capped { cap } =
            apply_anthropic_reasoning_budget(&mut final_req, state.max_thinking_tokens)
        {
            tracing::info!(
                model = %final_req.model,
                cap = cap,
                "reasoning budget: injected thinking.budget_tokens"
            );
        }
    }

    let mut resp = forward_provider(
        &state,
        &headers,
        ForwardOptions {
            path: "/v1/messages",
            payload_bytes: serde_json::to_vec(&final_req).unwrap_or_default(),
            request_streaming: processed.stream,
            cache_key,
            model: processed.model.clone(),
            format: StreamFormat::Anthropic,
            redaction_map,
            start_time,
            provider: "anthropic",
        }
    )
    .await;

    let tokens_used = state.budget.record(&scope_key, estimated_input_tokens);
    attach_guardrail_headers(&mut resp, injection_finding.as_ref(), tokens_used, &state.budget, &scope_key);
    resp
}


pub async fn handle_passthrough(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    let body = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return problem_response(StatusCode::BAD_REQUEST, "Invalid Request", &format!("read body: {err}"));
        }
    };

    if !uri.path().starts_with("/v1/") {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Err(err) =
        bridge_auth::require_bridge_token(state.bridge_token.as_deref(), &headers)
    {
        return err.into_response();
    }

    let headers = match bridge_auth::prepare_upstream_headers(
        state.bridge_token.as_deref(),
        state.upstream_api_key.as_deref(),
        &headers,
        UpstreamAuthStyle::Bearer,
    ) {
        Ok(h) => h,
        Err(err) => return err.into_response(),
    };

    let upstream_endpoint = format!("{}{}", state.upstream_url, uri);
    let upstream_req = with_forwarded_headers(
        state.http_client.request(method, upstream_endpoint).body(body),
        &headers,
    );

    match tokio::time::timeout(UPSTREAM_TIMEOUT, upstream_req.send()).await {
        Ok(Ok(resp)) => proxy_response(resp).await,
        Ok(Err(err)) => {
            error!(error = %err, "passthrough upstream error");
            problem_response(StatusCode::BAD_GATEWAY, "Upstream Error", "upstream unavailable")
        }
        Err(_) => {
            error!("passthrough upstream timeout");
            problem_response(StatusCode::GATEWAY_TIMEOUT, "Gateway Timeout", "upstream timeout")
        }
    }
}

fn apply_unified_middleware(
    state: &AppState,
    req: UnifiedRequest,
    scope: &Scope,
) -> (
    UnifiedRequest,
    UnifiedRequest,
    Option<Arc<RedactionMap>>,
) {
    let (redacted, map) = if state.enable_redaction {
        // Redaction for unified format would go here. For now, passthrough.
        // We could implement redact_unified_request(req)
        (req, None)
    } else {
        (req, None)
    };

    let cache_source = redacted.clone();
    let processed = if state.enable_compression {
        state.compressor.compress_unified_request(scope, redacted, state.enable_shrink)
    } else {
        redacted
    };

    (processed, cache_source, map)
}

fn unified_cache_key(
    state: &AppState,
    scope: &Scope,
    req: &UnifiedRequest,
    format_prefix: &str,
) -> String {
    if !state.enable_cache || !req.stream {
        return String::new();
    }
    let material = req.extract_cache_key_material(state.cache_key_strategy, state.cache_window_size);
    generate_cache_key(&scope.key(), &req.model, format_prefix, &material)
}

pub struct ForwardOptions {
    pub path: &'static str,
    pub payload_bytes: Vec<u8>,
    pub request_streaming: bool,
    pub cache_key: String,
    pub model: String,
    pub format: StreamFormat,
    pub redaction_map: Option<Arc<RedactionMap>>,
    pub start_time: Instant,
    pub provider: &'static str,
}

async fn forward_provider(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    opts: ForwardOptions,
) -> Response {
    if opts.request_streaming {
        let pipeline_opts = PipelineOptions {
            cache_key: opts.cache_key,
            model: opts.model.clone(),
            format: opts.format,
            redaction_map: opts.redaction_map,
            metrics: state.metrics.clone(),
        };

        let miss_stream = create_primed_miss_stream(
            Arc::clone(state),
            headers.clone(),
            opts.path.to_string(),
            opts.payload_bytes.clone(),
            pipeline_opts,
        );

        let instrumented = instrument_stream(
            miss_stream,
            state.metrics.clone(),
            opts.provider,
            opts.model.clone(),
            opts.path,
            true,
            "miss",
            opts.start_time,
        );

        return sse_stream_response(instrumented, None);
    }

    let base_url = get_upstream_url(state, &opts.model);
    let start_upstream = Instant::now();
    let auth_style = match opts.format {
        StreamFormat::OpenAI => UpstreamAuthStyle::Bearer,
        StreamFormat::Anthropic => UpstreamAuthStyle::AnthropicXApiKey,
    };
    let upstream_response =
        match post_with_failover(state, headers, base_url, opts.path, opts.payload_bytes, auth_style)
            .await
        {
        Ok(resp) => resp,
        Err(kind) => {
            state.metrics.record_error(opts.provider, kind);
            if kind == "misconfigured" {
                return bridge_auth::BridgeAuthError::UpstreamKeyRequired.into_response();
            }
            return problem_response(
                if kind == "timeout" {
                    StatusCode::GATEWAY_TIMEOUT
                } else {
                    StatusCode::BAD_GATEWAY
                },
                if kind == "timeout" { "Gateway Timeout" } else { "Upstream Error" },
                if kind == "timeout" {
                    "Upstream provider did not respond in time"
                } else {
                    "Upstream network failure"
                },
            );
        }
    };

    let status = upstream_response.status();
    state.metrics.record_upstream(opts.provider, status.as_u16(), start_upstream.elapsed());

    if !status.is_success() {
        let err_bytes = upstream_response.bytes().await.unwrap_or_default();
        state.metrics.record_error(opts.provider, "upstream");
        return (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK), err_bytes).into_response();
    }

    let full_bytes = upstream_response.bytes().await.unwrap_or_default();
    let elapsed = opts.start_time.elapsed();
    state.metrics.record_request(opts.provider, &opts.model, opts.path, false, "miss", elapsed);
    (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK), full_bytes).into_response()
}

fn sse_stream_response<S>(stream: S, cache_status: Option<&'static str>) -> Response
where
    S: futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    for (name, value) in SSE_HEADERS {
        headers.insert(name, HeaderValue::from_static(value));
    }
    if let Some(status) = cache_status {
        if let Ok(val) = HeaderValue::from_str(status) {
            headers.insert("x-kotro-cache", val);
        }
    }
    response
}

/// First-request path for `kotro-local-verify`: sync-store fixture SSE, stream as MISS.
fn serve_local_verify_miss(
    state: &Arc<AppState>,
    cache_key: String,
    model: String,
    redaction_map: Option<Arc<RedactionMap>>,
    injection_finding: Option<&InjectionFinding>,
    scope_key: &str,
    start_time: Instant,
) -> Response {
    let raw_sse = LOCAL_VERIFY_SSE.to_vec();
    let model_label = model.clone();
    let entry = Entry {
        key: cache_key,
        raw_sse: raw_sse.clone(),
        model,
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    };
    if let Err(err) = state.store.put(entry) {
        error!(error = %err, "local verify cache put failed");
        return problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Cache Store Failed",
            &format!("kotro-local-verify could not write cache entry: {err}"),
        );
    }
    state.metrics.record_cache_miss("openai");
    state.metrics.record_cache_store("openai");
    if let Ok(count) = state.store.count() {
        state.metrics.set_cache_entries(count);
    }

    let stream = create_cached_replay_stream(
        raw_sse,
        redaction_map,
        Duration::ZERO,
        StreamFormat::OpenAI,
        state.metrics.clone(),
    );
    // Replay helper is used for framing only — header must stay MISS on first write.
    let instrumented = instrument_stream(
        stream,
        state.metrics.clone(),
        "openai",
        model_label,
        "/v1/chat/completions",
        true,
        "miss",
        start_time,
    );
    let mut resp = sse_stream_response(instrumented, None);
    attach_guardrail_headers(
        &mut resp,
        injection_finding,
        state.budget.current(scope_key),
        &state.budget,
        scope_key,
    );
    resp
}


async fn proxy_response(upstream: reqwest::Response) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::OK);
    let headers = upstream.headers().clone();
    let body = upstream.bytes().await.unwrap_or_default();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    copy_response_headers(&headers, response.headers_mut());
    response
}

fn with_forwarded_headers(
    mut req: reqwest::RequestBuilder,
    src: &HeaderMap,
) -> reqwest::RequestBuilder {
    // Upstream OpenAI-compatible APIs (DeepSeek, OpenAI, …) require JSON.
    // `.body(bytes)` does not set this; without it a *valid* API key gets
    // HTTP 415 "Expected request with Content-Type: application/json" while
    // an invalid key fails earlier with 401 — which looks like a client bug.
    req = req.header(CONTENT_TYPE, "application/json");

    if let Some(auth) = src.get("authorization") {
        if let Ok(value) = auth.to_str() {
            req = req.header(AUTHORIZATION, value);
        }
    }
    for name in ["anthropic-version", "anthropic-beta", "x-api-key"] {
        if let Some(value) = src.get(name) {
            if let Ok(v) = value.to_str() {
                req = req.header(name, v);
            }
        }
    }
    req
}

fn copy_response_headers(src: &reqwest::header::HeaderMap, dst: &mut HeaderMap) {
    for (name, value) in src.iter() {
        if name == CONTENT_TYPE || name.as_str() == "content-length" {
            continue;
        }
        dst.insert(name, value.clone());
    }
    if let Some(ct) = src.get(CONTENT_TYPE) {
        dst.insert(CONTENT_TYPE, ct.clone());
    }
}

fn get_upstream_url<'a>(state: &'a AppState, model: &str) -> &'a str {
    // Local / MoE model → local upstream URL
    let is_moe = model == state.moe_default_model;
    if is_moe || state.local_model_pattern.as_ref().is_some_and(|p| p.is_match(model)) {
        if let Some(local_url) = &state.local_upstream_url {
            return local_url.as_str();
        }
    }
    // Cheap (Micro tier) model → dedicated cheap-model upstream URL (if configured)
    if let (Some(cheap_model), Some(cheap_url)) = (&state.cheap_model, &state.cheap_model_url) {
        if model == cheap_model.as_str() {
            return cheap_url.as_str();
        }
    }
    &state.upstream_url
}
