//! `korto-proxy` — single-binary local LLM reverse proxy (Rust Phase 2).

use korto_proxy::{config::Config, server::Server};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let cfg = Config::load();
    info!(
        service = "korto-proxy",
        phase = "4-router",
        upstream = %cfg.upstream_url,
        listen = %cfg.listen_addr,
        "starting kortolabs proxy"
    );

    let server = Server::new(cfg)?;
    server.run().await
}
