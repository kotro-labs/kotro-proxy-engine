//! Axum route handlers.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;
use tracing::{error, info};

use crate::cache::generate_cache_key;
use crate::compressor::Scope;
use crate::guardrail::{redact_chat_request, redact_messages_request, RedactionMap};
use crate::models::{anthropic::MessagesRequest, openai::ChatCompletionRequest};
use crate::proxy::pipeline::{create_processing_pipeline, PipelineOptions, StreamFormat};
use crate::proxy::replay::create_cached_replay_stream;
use crate::router::AppState;

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
        r#"{"status":"ok","service":"kortolabs-proxy"}"#,
    )
}

pub async fn handle_chat_completions(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let req: ChatCompletionRequest = match serde_json::from_value(payload) {
        Ok(req) => req,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, format!("invalid json: {err}")).into_response();
        }
    };

    let scope = state.scope.from_request(&headers, peer.ip());
    let (processed, cache_source, redaction_map) =
        apply_openai_middleware(&state, req, &scope);
    let cache_key = openai_cache_key(&state, &scope, &cache_source);

    if !cache_key.is_empty() {
        if let Ok(Some(entry)) = state.store.get(&cache_key) {
            info!(key = %cache_key, format = "openai", "cache hit");
            return sse_cached_response(
                entry.raw_sse,
                redaction_map,
                state.cache_hit_delay,
                true,
            );
        }
        info!(key = %cache_key, format = "openai", "cache miss");
    }

    forward_provider(
        &state,
        &headers,
        "/v1/chat/completions",
        &processed,
        processed.stream,
        cache_key,
        processed.model.clone(),
        StreamFormat::OpenAI,
        redaction_map,
    )
    .await
}

pub async fn handle_messages(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let req: MessagesRequest = match serde_json::from_value(payload) {
        Ok(req) => req,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, format!("invalid json: {err}")).into_response();
        }
    };

    let scope = state.scope.from_request(&headers, peer.ip());
    let (processed, cache_source, redaction_map) =
        apply_anthropic_middleware(&state, req, &scope);
    let cache_key = anthropic_cache_key(&state, &scope, &cache_source);

    if !cache_key.is_empty() {
        if let Ok(Some(entry)) = state.store.get(&cache_key) {
            info!(key = %cache_key, format = "anthropic", "cache hit");
            return sse_cached_response(
                entry.raw_sse,
                redaction_map,
                state.cache_hit_delay,
                true,
            );
        }
        info!(key = %cache_key, format = "anthropic", "cache miss");
    }

    forward_provider(
        &state,
        &headers,
        "/v1/messages",
        &processed,
        processed.stream,
        cache_key,
        processed.model.clone(),
        StreamFormat::Anthropic,
        redaction_map,
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
            return (StatusCode::BAD_REQUEST, format!("read body: {err}")).into_response();
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

    match upstream_req.send().await {
        Ok(resp) => proxy_response(resp).await,
        Err(err) => {
            error!(error = %err, "passthrough upstream error");
            (StatusCode::BAD_GATEWAY, "upstream unavailable").into_response()
        }
    }
}

fn apply_openai_middleware(
    state: &AppState,
    req: ChatCompletionRequest,
    scope: &Scope,
) -> (
    ChatCompletionRequest,
    ChatCompletionRequest,
    Option<Arc<RedactionMap>>,
) {
    let (redacted, map) = if state.enable_redaction {
        let (out, map) = redact_chat_request(req);
        (out, Some(map))
    } else {
        (req, None)
    };

    let cache_source = redacted.clone();
    let processed = if state.enable_compression {
        state.compressor.compress_chat_request(scope, redacted)
    } else {
        redacted
    };

    (processed, cache_source, map)
}

fn apply_anthropic_middleware(
    state: &AppState,
    req: MessagesRequest,
    scope: &Scope,
) -> (
    MessagesRequest,
    MessagesRequest,
    Option<Arc<RedactionMap>>,
) {
    let (redacted, map) = if state.enable_redaction {
        let (out, map) = redact_messages_request(req);
        (out, Some(map))
    } else {
        (req, None)
    };

    let cache_source = redacted.clone();
    let processed = if state.enable_compression {
        state.compressor.compress_messages_request(scope, redacted)
    } else {
        redacted
    };

    (processed, cache_source, map)
}

fn openai_cache_key(
    state: &AppState,
    scope: &Scope,
    req: &ChatCompletionRequest,
) -> String {
    if !state.enable_cache || !req.stream {
        return String::new();
    }
    let material = req.extract_cache_key_material(state.cache_key_strategy, state.cache_window_size);
    generate_cache_key(&scope.key(), &req.model, "openai", &material)
}

fn anthropic_cache_key(
    state: &AppState,
    scope: &Scope,
    req: &MessagesRequest,
) -> String {
    if !state.enable_cache || !req.stream {
        return String::new();
    }
    let material = req.extract_cache_key_material(state.cache_key_strategy, state.cache_window_size);
    generate_cache_key(&scope.key(), &req.model, "anthropic", &material)
}

async fn forward_provider<T: serde::Serialize>(
    state: &AppState,
    headers: &HeaderMap,
    path: &str,
    payload: &T,
    request_streaming: bool,
    cache_key: String,
    model: String,
    format: StreamFormat,
    redaction_map: Option<Arc<RedactionMap>>,
) -> Response {
    let upstream_endpoint = format!("{}{}", state.upstream_url, path);
    let upstream_req = with_forwarded_headers(
        state.http_client.post(upstream_endpoint).json(payload),
        headers,
    );

    let upstream_response = match upstream_req.send().await {
        Ok(resp) => resp,
        Err(err) => {
            error!(error = %err, "upstream network failure");
            return (
                StatusCode::BAD_GATEWAY,
                format!("Upstream network failure: {err}"),
            )
                .into_response();
        }
    };

    let status =
        StatusCode::from_u16(upstream_response.status().as_u16()).unwrap_or(StatusCode::OK);
    if !status.is_success() {
        let err_bytes = upstream_response.bytes().await.unwrap_or_default();
        return (status, err_bytes).into_response();
    }

    let upstream_sse = upstream_response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("text/event-stream"));

    if request_streaming && upstream_sse {
        let upstream_byte_stream = upstream_response.bytes_stream();
        let pipeline_opts = PipelineOptions {
            cache_key,
            model,
            format,
            redaction_map,
        };

        let outbound = create_processing_pipeline(
            upstream_byte_stream,
            state.store.clone(),
            pipeline_opts,
        );

        return sse_stream_response(outbound, false);
    }

    let full_bytes = upstream_response.bytes().await.unwrap_or_default();
    (status, full_bytes).into_response()
}

fn sse_cached_response(
    raw_sse: Vec<u8>,
    redaction_map: Option<Arc<RedactionMap>>,
    hit_delay: Duration,
    cache_hit: bool,
) -> Response {
    let stream = create_cached_replay_stream(raw_sse, redaction_map, hit_delay);
    sse_stream_response(stream, cache_hit)
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
        headers.insert("x-kortolabs-cache", HeaderValue::from_static("HIT"));
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
