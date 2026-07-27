//! Axum listener bootstrap — mirrors `internal/server/server.go`.

use std::net::SocketAddr;

use axum::Router;
use tokio::signal;
use tracing::info;

use crate::cache::{start_eviction_worker, Store};
use crate::config::Config;
use crate::router::{build_http_client, create_router, create_telemetry_router, open_store, AppState};


pub struct Server {
    cfg: Config,
    store: Store,
    router: Router,
    telemetry_router: Option<Router>,
}

impl Server {
    pub fn new(cfg: Config) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let store = open_store(&cfg)?;
        start_eviction_worker(store.clone(), cfg.eviction_interval);
        
        let metrics = crate::metrics::MetricsRegistry::new()
            .with_dashboard_usd_per_token(cfg.dashboard_usd_per_token);
        metrics.set_cache_key_strategy(&format!("{:?}", cfg.cache_key_strategy), cfg.cache_window_size);
        if let Ok(count) = store.count() {
            metrics.set_cache_entries(count);
        }

        let client = build_http_client()?;
        let state = AppState::new(&cfg, store.clone(), client, metrics.clone());
        let router = create_router(state.clone());
        
        let telemetry_router = if cfg.enable_metrics {
            Some(create_telemetry_router(state))
        } else {
            None
        };

        Ok(Self {
            cfg,
            store,
            router,
            telemetry_router,
        })
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let proxy_addr = normalize_listen_addr(&self.cfg.listen_addr);
        let proxy_listener = tokio::net::TcpListener::bind(&proxy_addr).await?;
        let proxy_local = proxy_listener.local_addr()?;

        let proxy_service = self.router.into_make_service_with_connect_info::<SocketAddr>();
        let proxy_server = axum::serve(proxy_listener, proxy_service)
            .with_graceful_shutdown(shutdown_signal());

        if self.cfg.enable_metrics {
            if let Some(telemetry_router) = self.telemetry_router.take() {
            let metrics_addr = resolve_control_addr(&self.cfg.metrics_addr);
            let metrics_listener = tokio::net::TcpListener::bind(&metrics_addr).await?;
            let metrics_local = metrics_listener.local_addr()?;

            info!(
                addr = %proxy_local,
                metrics_addr = %metrics_local,
                metrics_enabled = true,
                upstream = %self.cfg.upstream_url,
                cache_db = %self.cfg.cache_db_path,
                cache = self.cfg.enable_cache,
                redaction = self.cfg.enable_redaction,
                cache_ttl_secs = self.cfg.cache_ttl.as_secs(),
                cache_eviction_secs = self.cfg.eviction_interval.as_secs(),
                "kotrolabs proxy listening"
            );

            let metrics_service = telemetry_router.into_make_service_with_connect_info::<SocketAddr>();
            let metrics_server = axum::serve(metrics_listener, metrics_service)
                .with_graceful_shutdown(shutdown_signal());

            tokio::select! {
                res = proxy_server => {
                    if let Err(err) = res {
                        tracing::error!(error = %err, "proxy server error");
                    }
                }
                res = metrics_server => {
                    if let Err(err) = res {
                        tracing::error!(error = %err, "metrics server error");
                    }
                }
                }
            }
        } else {
            info!(
                addr = %proxy_local,
                metrics_enabled = false,
                upstream = %self.cfg.upstream_url,
                cache_db = %self.cfg.cache_db_path,
                cache = self.cfg.enable_cache,
                redaction = self.cfg.enable_redaction,
                cache_ttl_secs = self.cfg.cache_ttl.as_secs(),
                cache_eviction_secs = self.cfg.eviction_interval.as_secs(),
                "kotrolabs proxy listening"
            );

            if let Err(err) = proxy_server.await {
                tracing::error!(error = %err, "proxy server error");
            }
        }

        drop(self.store);
        Ok(())
    }
}


fn normalize_listen_addr(addr: &str) -> SocketAddr {
    if let Ok(parsed) = addr.parse::<SocketAddr>() {
        return parsed;
    }
    if let Some(stripped) = addr.strip_prefix(':') {
        if let Ok(port) = stripped.parse::<u16>() {
            return SocketAddr::from(([0, 0, 0, 0], port));
        }
    }
    addr.parse().unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 8080)))
}

/// Resolve the control/telemetry listener address with **strict loopback
/// binding**.
///
/// This listener carries the control API (kill switch, approvals, action-plane
/// event ingestion) alongside read-only metrics. A bare `:9090` would otherwise
/// normalize to `0.0.0.0` and publish those endpoints to the LAN, so any
/// non-loopback address is coerced back to loopback.
///
/// `KOTRO_ALLOW_REMOTE_CONTROL=true` is a deliberate, loudly-warned escape
/// hatch for users who front the listener with their own authenticated tunnel.
fn resolve_control_addr(addr: &str) -> SocketAddr {
    let resolved = normalize_listen_addr(addr);
    if resolved.ip().is_loopback() {
        return resolved;
    }
    let allow_remote = std::env::var("KOTRO_ALLOW_REMOTE_CONTROL")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    if allow_remote {
        tracing::warn!(
            requested = %resolved,
            "KOTRO_ALLOW_REMOTE_CONTROL is set — the control API (kill switch, approvals, \
             event ingestion) is reachable off-host. Ensure an authenticated tunnel fronts it."
        );
        return resolved;
    }
    let coerced = SocketAddr::from(([127, 0, 0, 1], resolved.port()));
    tracing::warn!(
        requested = %resolved,
        bound = %coerced,
        "control API refused a non-loopback bind; coerced to loopback. \
         Set KOTRO_ALLOW_REMOTE_CONTROL=true to override."
    );
    coerced
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bare_port() {
        let addr = normalize_listen_addr(":8080");
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn control_addr_coerces_non_loopback_to_loopback() {
        // A bare port would normalize to 0.0.0.0 and expose the control API.
        let addr = resolve_control_addr(":9090");
        assert!(addr.ip().is_loopback(), "got {addr}");
        assert_eq!(addr.port(), 9090);

        // An explicit external bind is also refused.
        let addr = resolve_control_addr("0.0.0.0:9191");
        assert!(addr.ip().is_loopback(), "got {addr}");
        assert_eq!(addr.port(), 9191);
    }

    #[test]
    fn control_addr_preserves_explicit_loopback() {
        let addr = resolve_control_addr("127.0.0.1:9090");
        assert_eq!(addr.to_string(), "127.0.0.1:9090");
    }
}
