//! Cache-hit SSE replay — mirrors `internal/proxy/stream.go` `replayCached`.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_stream::try_stream;
use bytes::Bytes;
use futures_util::Stream;
use tokio::time::sleep;

use crate::guardrail::{restore_payload, RedactionMap};
use crate::proxy::bootstrap::bootstrap_bytes;
use crate::sse::frame::{transform_data_line, Frame, ReaderError};
use crate::sse::Reader;

fn client_frame_bytes(
    frame: &Frame,
    redaction_map: Option<&Arc<RedactionMap>>,
) -> Bytes {
    if let Some(map) = redaction_map.filter(|m| !m.is_empty()) {
        let map = Arc::clone(map);
        return transform_data_line(frame, |payload| restore_payload(payload, &map)).to_bytes();
    }
    frame.to_bytes()
}

/// Streams cached SSE frames with bootstrap priming and optional pacing delay.
pub fn create_cached_replay_stream(
    raw_sse: Vec<u8>,
    redaction_map: Option<Arc<RedactionMap>>,
    hit_delay: Duration,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send + 'static>> {
    Box::pin(try_stream! {
        yield bootstrap_bytes();

        let mut reader = Reader::new();
        reader.feed(&raw_sse);
        reader.mark_eof();

        loop {
            match reader.next() {
                Ok(frame) => {
                    yield client_frame_bytes(&frame, redaction_map.as_ref());
                    if !hit_delay.is_zero() {
                        sleep(hit_delay).await;
                    }
                }
                Err(ReaderError::Eof) => break,
                Err(ReaderError::NeedMoreData) => break,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn replays_cached_frames_with_bootstrap() {
        let raw = b"data: {\"x\":1}\n\ndata: [DONE]\n\n".to_vec();
        let mut stream = create_cached_replay_stream(raw, None, Duration::ZERO);
        let first = stream.next().await.unwrap().unwrap();
        assert!(first.starts_with(b": kortolabs"));
    }
}
