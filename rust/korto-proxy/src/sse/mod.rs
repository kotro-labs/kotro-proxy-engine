//! SSE frame parser — mirrors `internal/sse/stream.go` (Phase 2.3).

pub mod frame;

pub use frame::{
    parse_frame_bytes, transform_data_line, write_frame, Frame, FrameParseResult, Reader,
    ReaderError, SseFrameParser,
};
