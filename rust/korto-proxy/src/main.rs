//! `korto-proxy` — single-binary local LLM reverse proxy (Rust Phase 2).

use korto_proxy::{config::Config, server::Server};
use std::env;
use tracing::info;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("korto-proxy {VERSION}");
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let cfg = Config::load();
    info!(
        service = "korto-proxy",
        listen = %cfg.listen_addr,
        metrics = %cfg.metrics_addr,
        upstream = %cfg.upstream_url,
        fallback_configured = cfg.fallback_url.is_some(),
        profile = %env::var("KORTO_PROFILE").unwrap_or_default(),
        cache_strategy = ?cfg.cache_key_strategy,
        cache_window = cfg.cache_window_size,
        redaction = cfg.enable_redaction,
        compression = cfg.enable_compression,
        "starting kortolabs proxy"
    );

    let server = Server::new(cfg)?;
    server.run().await
}
