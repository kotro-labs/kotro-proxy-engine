//! Reverse-proxy pipeline — axum handlers land in Phase 2.5.

pub mod bootstrap;
pub mod pipeline;
pub mod replay;

pub use pipeline::{create_processing_pipeline, PipelineOptions, StreamFormat};
pub use replay::create_cached_replay_stream;
