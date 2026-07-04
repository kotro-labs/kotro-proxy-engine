//! Korto Proxy Engine — Rust Phase 2
//!
//! Semantic SSE cache, PII guardrail, and context compression for local LLM agents.
//! Go Phase 1 on `main` is the behavioral reference implementation.

pub mod cache;
pub mod config;
pub mod models;
pub mod router;
pub mod server;
pub mod sse;

pub mod compressor;
pub mod guardrail;
pub mod proxy;
pub mod metrics;
pub mod dashboard_assets;


pub use config::Config;
pub use sse::{Frame, Reader, SseFrameParser};
