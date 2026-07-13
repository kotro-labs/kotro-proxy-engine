#![allow(clippy::result_large_err)]
//! Axum HTTP/2 router — mirrors `internal/server/server.go` + handlers.

mod handlers;
pub mod scope;
pub mod upstream;
pub mod classifier;

use std::sync::Arc;
use std::time::Duration;

use axum::{
    routing::{get, post},
    Router,
};
use reqwest::Client;

use crate::cache::{Store, StoreOptions, CacheKeyStrategy};
use crate::compressor::StateTracker;
use crate::config::Config;
use crate::router::scope::{parse_trusted_cidrs, ScopeResolver};

use handlers::{
    handle_chat_completions, handle_healthz, handle_messages, handle_passthrough,
    handle_api_dashboard, handle_dashboard, handle_icon, handle_metrics,
};

#[derive(Clone)]
pub struct AppState {

    pub store: Store,
    pub http_client: Client,
    pub upstream_url: String,
    pub fallback_url: Option<String>,
    pub fallback_model: Option<String>,
    pub enable_cache: bool,
    pub enable_redaction: bool,
    pub enable_compression: bool,
    pub enable_shrink: bool,
    pub cache_hit_delay: Duration,
    pub compressor: Arc<StateTracker>,
    pub scope: ScopeResolver,
    pub cache_key_strategy: CacheKeyStrategy,
    pub cache_window_size: usize,
    pub metrics: crate::metrics::MetricsRegistry,
    pub local_model_pattern: Option<regex::Regex>,
    pub local_upstream_url: Option<String>,
    pub moe_default_model: String,
    /// Model name for the `Micro` complexity tier (cheap/fast API model).
    pub cheap_model: Option<String>,
    /// Upstream base URL for cheap model requests. `None` = same as `upstream_url`.
    pub cheap_model_url: Option<String>,
    /// Number of identical tool calls before the per-conversation loop CB fires.
    /// 0 = disabled.
    pub tool_loop_threshold: u32,
    pub vector_encoder: Arc<crate::cache::vector::SemanticEncoder>,
    pub vector_index: Arc<crate::cache::vector::VectorIndex>,
    pub circuit_breaker: moka::sync::Cache<String, u32>,
    /// Run injection scanner on tool-call results and user messages.
    pub enable_injection_scan: bool,
    /// Block (HTTP 400) when injection is detected, rather than just warning.
    pub injection_block_on_detection: bool,
    /// Per-scope session token budget tracker.
    pub budget: Arc<crate::budget::BudgetTracker>,
    /// Maximum thinking/reasoning tokens per request for known reasoning models.
    /// `0` = no cap.
    pub max_thinking_tokens: u64,
    /// When `true`, requests to reasoning models are rejected with HTTP 403
    /// instead of having their thinking budget capped.
    pub reasoning_block: bool,
}

impl AppState {
    pub fn new(cfg: &Config, store: Store, http_client: Client, metrics: crate::metrics::MetricsRegistry) -> Self {
        let trusted_cidrs = match parse_trusted_cidrs(&cfg.trusted_proxy_cidrs) {
            Ok(cidrs) => cidrs,
            Err(err) => {
                tracing::error!(
                    error = %err,
                    value = %cfg.trusted_proxy_cidrs,
                    "invalid KOTRO_TRUSTED_PROXY_CIDRS; failing safe with empty trusted-proxy whitelist"
                );
                Vec::new()
            }
        };
        Self {
            store,
            http_client,
            upstream_url: cfg.upstream_url.trim_end_matches('/').to_string(),
            fallback_url: cfg.fallback_url.clone().map(|u| u.trim_end_matches('/').to_string()),
            fallback_model: cfg.fallback_model.clone(),
            enable_cache: cfg.enable_cache,
            enable_redaction: cfg.enable_redaction,
            enable_compression: cfg.enable_compression,
            enable_shrink: cfg.enable_shrink,
            cache_hit_delay: cfg.cache_hit_delay,
            compressor: Arc::new(StateTracker::new(
                cfg.compressor_max_scopes,
                cfg.compressor_scope_ttl,
            )),
            scope: ScopeResolver {
                trust_upstream_gateway: cfg.trust_upstream_gateway,
                trusted_proxy_cidrs: trusted_cidrs,
            },
            cache_key_strategy: cfg.cache_key_strategy,
            cache_window_size: cfg.cache_window_size,
            metrics,
            local_model_pattern: cfg.local_model_pattern.as_ref().and_then(|p| regex::Regex::new(p).ok()),
            local_upstream_url: cfg.local_upstream_url.clone().map(|u| u.trim_end_matches('/').to_string()),
            moe_default_model: cfg.moe_default_model.clone(),
            cheap_model: cfg.cheap_model.clone(),
            cheap_model_url: cfg.cheap_model_url.clone().map(|u| u.trim_end_matches('/').to_string()),
            tool_loop_threshold: cfg.tool_loop_threshold,
            vector_encoder: Arc::new(crate::cache::vector::SemanticEncoder::new(cfg.enable_vector_cache)),
            vector_index: Arc::new(crate::cache::vector::VectorIndex::new()),
            circuit_breaker: moka::sync::Cache::builder()
                .time_to_live(Duration::from_secs(60))
                .build(),
            enable_injection_scan: cfg.enable_injection_scan,
            injection_block_on_detection: cfg.injection_block_on_detection,
            budget: Arc::new(crate::budget::BudgetTracker::new(
                cfg.session_token_budget,
                cfg.budget_block_on_exceeded,
                std::time::Duration::from_secs(86_400),
            )),
            max_thinking_tokens: cfg.max_thinking_tokens,
            reasoning_block: cfg.reasoning_block,
        }
    }
}


pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(handle_healthz))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/messages", post(handle_messages))
        .fallback(handle_passthrough)
        .with_state(Arc::new(state))
}

pub fn create_telemetry_router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(handle_metrics))
        .route("/dashboard", get(handle_dashboard))
        .route("/api/dashboard", get(handle_api_dashboard))
        .route("/favicon.ico", get(handle_icon))
        .route("/dashboard/icon.png", get(handle_icon))
        .with_state(Arc::new(state))
}


pub fn open_store(cfg: &Config) -> Result<Store, crate::cache::StoreError> {
    Store::open_with_options(
        &cfg.cache_db_path,
        StoreOptions {
            ttl: cfg.cache_ttl,
            enable_compression: cfg.enable_compression,
            max_capacity: None,
        },
    )
}

pub fn build_http_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .pool_max_idle_per_host(8)
        .tcp_keepalive(Some(std::time::Duration::from_secs(60)))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_ok() {
        let cfg = Config::default();
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = cfg;
        cfg.cache_db_path = dir.path().join("cache.db").display().to_string();

        let store = open_store(&cfg).unwrap();
        let client = build_http_client().unwrap();
        let app = create_router(AppState::new(&cfg, store, client, crate::metrics::MetricsRegistry::new()));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(body.windows(2).any(|w| w == b"ok"));
    }
}
