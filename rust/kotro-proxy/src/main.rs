//! `kotro-proxy` — single-binary local LLM reverse proxy (Rust Phase 2).

use kotro_proxy::{config::Config, server::Server};
use std::env;
use tracing::info;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("kotro-proxy {VERSION}");
        return Ok(());
    }

    let cfg = Config::load();

    // Initialise telemetry and retain the provider handle so we can flush
    // buffered spans before exit (only Some when KOTRO_OTEL_ENDPOINT is set).
    let otel_provider = match kotro_proxy::telemetry::otel::init_telemetry(cfg.otel_endpoint.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to initialize telemetry: {e}");
            None
        }
    };

    info!(
        service = "kotro-proxy",
        listen = %cfg.listen_addr,
        metrics = %cfg.metrics_addr,
        upstream = %cfg.upstream_url,
        fallback_configured = cfg.fallback_url.is_some(),
        profile = %env::var("KOTRO_PROFILE").unwrap_or_default(),
        cache_strategy = ?cfg.cache_key_strategy,
        cache_window = cfg.cache_window_size,
        redaction = cfg.enable_redaction,
        compression = cfg.enable_compression,
        "starting kotrolabs proxy"
    );

    let server = Server::new(cfg)?;
    server.run().await?;

    // Flush any buffered OTel spans before the process exits.
    if let Some(provider) = otel_provider {
        if let Err(e) = provider.shutdown() {
            eprintln!("OTel provider shutdown error: {e}");
        }
    }

    Ok(())
}
