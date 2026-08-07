//! Kotro Proxy Engine — Rust Phase 2
//!
//! Exact prompt-state SSE cache, optional MiniLM semantic layer, PII guardrail,
//! agent flight recorder / kill switch, and context compression for local LLM agents.
//! Go Phase 1 on `main` is the behavioral reference implementation.

pub mod cache;
pub mod config;
pub mod models;
pub mod router;
pub mod server;
pub mod sse;

pub mod budget;
pub mod compressor;
pub mod corpus;
pub mod flight_recorder;
pub mod graph;
pub mod guardrail;
pub mod hook;
pub mod identity_ctx;
pub mod isolate;
pub mod mcp;
pub mod optimizer;
pub mod policy;
pub mod posture;
pub mod proxy;
pub mod metrics;
pub mod dashboard_assets;
pub mod plugins;
pub mod telemetry;
pub mod numbat;
pub mod permit;

pub use config::Config;
pub use sse::{Frame, Reader, SseFrameParser};
