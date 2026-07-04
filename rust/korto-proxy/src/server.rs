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
        
        let metrics = crate::metrics::MetricsRegistry::new();
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

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let proxy_addr = normalize_listen_addr(&self.cfg.listen_addr);
        let proxy_listener = tokio::net::TcpListener::bind(&proxy_addr).await?;
        let proxy_local = proxy_listener.local_addr()?;

        let proxy_service = self.router.into_make_service_with_connect_info::<SocketAddr>();
        let proxy_server = axum::serve(proxy_listener, proxy_service)
            .with_graceful_shutdown(shutdown_signal());

        if self.cfg.enable_metrics && self.telemetry_router.is_some() {
            let metrics_addr = normalize_listen_addr(&self.cfg.metrics_addr);
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
                "kortolabs proxy listening"
            );

            let metrics_service = self.telemetry_router.unwrap().into_make_service_with_connect_info::<SocketAddr>();
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
                "kortolabs proxy listening"
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
}
