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
use std::time::Instant;
use tracing::{error, info};

use crate::cache::generate_cache_key;
use crate::compressor::Scope;
use crate::guardrail::{redact_chat_request, redact_messages_request, RedactionMap};
use crate::models::{anthropic::MessagesRequest, openai::ChatCompletionRequest, unified::UnifiedRequest};
use crate::router::classifier;
use crate::proxy::pipeline::{create_processing_pipeline, PipelineOptions, StreamFormat};
use crate::proxy::replay::create_cached_replay_stream;
use crate::router::AppState;
use crate::router::upstream;

const UPSTREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

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
        let upstream_response = match post_with_failover(&state, &headers, base_url, &path, payload_bytes.clone()).await {
            Ok(resp) => resp,
            Err(kind) => {
                let provider_str = match pipeline_opts.format {
                    StreamFormat::OpenAI => "openai",
                    StreamFormat::Anthropic => "anthropic",
                };
                state.metrics.record_error(provider_str, kind);
                let err_msg = match kind {
                    "timeout" => "data: {\"error\": \"Gateway timeout: Upstream provider did not respond in time\"}\n\n".to_string(),
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
) -> Result<reqwest::Response, &'static str> {
    let primary = format!("{}{}", primary_base, path);
    let primary_req =
        with_forwarded_headers(state.http_client.post(primary).body(body.clone()), headers);

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
        with_forwarded_headers(state.http_client.post(fallback).body(body), headers);

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
    let start_time = Instant::now();
    let mut req: ChatCompletionRequest = match serde_json::from_value(payload) {
        Ok(req) => req,
        Err(err) => {
            state.metrics.record_error("openai", "parse");
            return problem_response(StatusCode::BAD_REQUEST, "Invalid Request", &format!("invalid json: {err}"));
        }
    };

    crate::optimizer::enforce_cache_matrix(&mut req);

    let body_str = serde_json::to_string(&req).unwrap_or_default();
    state.metrics.record_request_body("openai", body_str.len());

    let scope = state.scope.from_request(&headers, peer.ip());
    let (_, latest_user) = req.extract_prompt_state();

    if classifier::is_trivial_prompt(&latest_user) && state.local_upstream_url.is_some() {
        tracing::info!("MoE triggered: routed trivial prompt to {}", state.moe_default_model);
        req.model = state.moe_default_model.clone();
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
                "/v1/chat/completions",
                true,
                "hit",
                start_time,
            );
            return sse_stream_response(instrumented, true);
        }
        info!(key = %cache_key, format = "openai", "cache miss");
        
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
                        "/v1/chat/completions",
                        true,
                        "hit",
                        start_time,
                    );
                    return sse_stream_response(instrumented, true);
                }
            }
            state.vector_index.insert(context_key, cache_key.clone(), latest_user.clone(), user_emb);
        }

        state.metrics.record_cache_miss("openai");
        
        let count = state.circuit_breaker.get(&cache_key).unwrap_or(0) + 1;
        state.circuit_breaker.insert(cache_key.clone(), count);
        if count >= 4 {
            tracing::warn!(key = %cache_key, count = count, "circuit breaker tripped");
            if processed.stream {
                let err_msg = "data: {\"choices\": [{\"delta\": {\"content\": \"\\n\\n🚨 [KOTRO CIRCUIT BREAKER TRIPPED]: Infinite error loop detected. Halting execution to prevent API credit drain. Please ask the human operator for guidance.\"}}]}\n\ndata: [DONE]\n\n";
                let stream = futures_util::stream::once(async move { Ok::<_, io::Error>(Bytes::from(err_msg)) });
                return sse_stream_response(stream, false);
            } else {
                return problem_response(StatusCode::TOO_MANY_REQUESTS, "Circuit Breaker Tripped", "Infinite error loop detected. Halting execution to prevent API credit drain.");
            }
        }
    }

    let final_req: ChatCompletionRequest = processed.clone().into();

    forward_provider(
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
    .await
}

pub async fn handle_messages(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let start_time = Instant::now();
    let req: MessagesRequest = match serde_json::from_value(payload) {
        Ok(req) => req,
        Err(err) => {
            state.metrics.record_error("anthropic", "parse");
            return problem_response(StatusCode::BAD_REQUEST, "Invalid Request", &format!("invalid json: {err}"));
        }
    };

    let body_str = serde_json::to_string(&req).unwrap_or_default();
    state.metrics.record_request_body("anthropic", body_str.len());

    let scope = state.scope.from_request(&headers, peer.ip());
    let (_, latest_user) = req.extract_prompt_state();

    let unified_req: UnifiedRequest = req.clone().try_into().unwrap_or_else(|_| {
        UnifiedRequest {
            model: req.model.clone(),
            system_prompt: "".into(),
            messages: vec![],
            stream: req.stream,
            max_tokens: Some(req.max_tokens),
        }
    });

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
                "/v1/messages",
                true,
                "hit",
                start_time,
            );
            return sse_stream_response(instrumented, true);
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
                        "/v1/messages",
                        true,
                        "hit",
                        start_time,
                    );
                    return sse_stream_response(instrumented, true);
                }
            }
            state.vector_index.insert(context_key, cache_key.clone(), latest_user.clone(), user_emb);
        }

        state.metrics.record_cache_miss("anthropic");

        let count = state.circuit_breaker.get(&cache_key).unwrap_or(0) + 1;
        state.circuit_breaker.insert(cache_key.clone(), count);
        if count >= 4 {
            tracing::warn!(key = %cache_key, count = count, "circuit breaker tripped");
            if processed.stream {
                let err_msg = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"\\n\\n🚨 [KOTRO CIRCUIT BREAKER TRIPPED]: Infinite error loop detected. Halting execution to prevent API credit drain. Please ask the human operator for guidance.\"}}\n\n";
                let stream = futures_util::stream::once(async move { Ok::<_, io::Error>(Bytes::from(err_msg)) });
                return sse_stream_response(stream, false);
            } else {
                return problem_response(StatusCode::TOO_MANY_REQUESTS, "Circuit Breaker Tripped", "Infinite error loop detected. Halting execution to prevent API credit drain.");
            }
        }
    }

    let final_req: MessagesRequest = processed.clone().into();

    forward_provider(
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
    .await
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
            opts.path,
            true,
            "miss",
            opts.start_time,
        );

        return sse_stream_response(instrumented, false);
    }

    let base_url = get_upstream_url(state, &opts.model);
    let start_upstream = Instant::now();
    let upstream_response = match post_with_failover(state, headers, base_url, opts.path, opts.payload_bytes).await {
        Ok(resp) => resp,
        Err(kind) => {
            state.metrics.record_error(opts.provider, kind);
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
    state.metrics.record_request(opts.provider, opts.path, false, "miss", elapsed);
    (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK), full_bytes).into_response()
}

fn sse_stream_response<S>(stream: S, cache_hit: bool) -> Response
where
    S: futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    for (name, value) in SSE_HEADERS {
        headers.insert(name, HeaderValue::from_static(value));
    }
    if cache_hit {
        headers.insert("x-kotro-cache", HeaderValue::from_static("HIT"));
    }
    response
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
    let is_moe = model == state.moe_default_model;
    if is_moe || state.local_model_pattern.as_ref().is_some_and(|p| p.is_match(model)) {
        if let Some(local_url) = &state.local_upstream_url {
            return local_url.as_str();
        }
    }
    &state.upstream_url
}
