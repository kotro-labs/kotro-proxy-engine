//! Cache-hit SSE replay — mirrors `internal/proxy/stream.go` `replayCached`.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_stream::try_stream;
use bytes::Bytes;
use futures_util::Stream;
use tokio::time::sleep;

use crate::guardrail::RedactionMap;
use crate::proxy::bootstrap::bootstrap_bytes;
use crate::proxy::pipeline::StreamFormat;
use crate::sse::frame::{transform_data_line, Frame, ReaderError};
use crate::sse::Reader;

fn client_frame_bytes(
    frame: &Frame,
    redaction_map: Option<&Arc<RedactionMap>>,
    format: StreamFormat,
    metrics: &crate::metrics::MetricsRegistry,
) -> Bytes {
    if let Some(map) = redaction_map.filter(|m| !m.is_empty()) {
        let map = Arc::clone(map);
        let metrics = metrics.clone();
        return transform_data_line(frame, |payload| {
            let (restored, count) = crate::guardrail::restore_payload_counted(payload, &map, format);
            metrics.record_redaction_restores(count);
            restored
        }).to_bytes();
    }
    frame.to_bytes()
}

/// Streams cached SSE frames with bootstrap priming and optional pacing delay.
pub fn create_cached_replay_stream(
    raw_sse: Vec<u8>,
    redaction_map: Option<Arc<RedactionMap>>,
    hit_delay: Duration,
    format: StreamFormat,
    metrics: crate::metrics::MetricsRegistry,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send + 'static>> {
    Box::pin(try_stream! {
        yield bootstrap_bytes();

        let mut reader = Reader::new();
        reader.feed(&raw_sse);
        reader.mark_eof();

        loop {
            match reader.next() {
                Ok(frame) => {
                    yield client_frame_bytes(&frame, redaction_map.as_ref(), format, &metrics);
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
        let mut stream = create_cached_replay_stream(
            raw,
            None,
            Duration::ZERO,
            StreamFormat::OpenAI,
            crate::metrics::MetricsRegistry::new(),
        );
        let first = stream.next().await.unwrap().unwrap();
        assert!(first.starts_with(b": kortolabs"));
    }
}
