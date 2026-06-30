//! Axum listener bootstrap — mirrors `internal/server/server.go`.

use std::net::SocketAddr;

use axum::Router;
use tokio::signal;
use tracing::info;

use crate::cache::Store;
use crate::config::Config;
use crate::router::{build_http_client, create_router, open_store, AppState};

pub struct Server {
    cfg: Config,
    store: Store,
    router: Router,
}

impl Server {
    pub fn new(cfg: Config) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let store = open_store(&cfg)?;
        let client = build_http_client()?;
        let state = AppState::new(&cfg, store.clone(), client);
        let router = create_router(state);
        Ok(Self {
            cfg,
            store,
            router,
        })
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = normalize_listen_addr(&self.cfg.listen_addr);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        let local = listener.local_addr()?;

        info!(
            addr = %local,
            upstream = %self.cfg.upstream_url,
            cache_db = %self.cfg.cache_db_path,
            cache = self.cfg.enable_cache,
            redaction = self.cfg.enable_redaction,
            cache_ttl_secs = self.cfg.cache_ttl.as_secs(),
            "kortolabs proxy listening"
        );

        axum::serve(listener, self.router)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

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
