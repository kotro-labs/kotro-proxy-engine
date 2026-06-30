//! Non-blocking asynchronous tee topology — mirrors `internal/proxy/stream.go`.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_stream::try_stream;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use tracing::{debug, error};

use crate::cache::{Entry, Store};
use crate::guardrail::{restore_payload, RedactionMap};
use crate::proxy::bootstrap::bootstrap_bytes;
use crate::sse::frame::{parse_frame_bytes, transform_data_line, Frame, FrameParseResult};
use crate::sse::SseFrameParser;

/// Provider-specific SSE completion semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFormat {
    OpenAI,
    Anthropic,
}

/// Configuration for the cache-miss streaming interceptor.
#[derive(Clone)]
pub struct PipelineOptions {
    pub cache_key: String,
    pub model: String,
    pub format: StreamFormat,
    pub redaction_map: Option<Arc<RedactionMap>>,
}

fn frame_complete(frame: &Frame, format: StreamFormat) -> bool {
    match format {
        StreamFormat::OpenAI => frame.is_openai_done(),
        StreamFormat::Anthropic => frame.is_anthropic_complete(),
    }
}

fn client_frame_bytes(frame: &Frame, opts: &PipelineOptions) -> Bytes {
    if let Some(map) = &opts.redaction_map {
        if !map.is_empty() {
            let map = Arc::clone(map);
            return transform_data_line(frame, |payload| restore_payload(payload, &map)).to_bytes();
        }
    }
    frame.to_bytes()
}

/// Transforms an upstream byte stream: bootstrap → frame parse → optional restore → cache tee.
///
/// Dropping the returned stream before completion skips the background cache write (RAII).
pub fn create_processing_pipeline<S, E>(
    upstream_stream: S,
    store: Store,
    opts: PipelineOptions,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send + 'static>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + Unpin + 'static,
    E: std::fmt::Display + Send + 'static,
{
    Box::pin(try_stream! {
        let mut upstream_stream = upstream_stream;
        let mut parser = SseFrameParser::new();
        let mut cache_accumulator = Vec::new();
        let mut is_complete = false;

        yield bootstrap_bytes();

        while let Some(chunk_result) = upstream_stream.next().await {
            let raw_chunk = chunk_result.map_err(|e| {
                io::Error::new(io::ErrorKind::ConnectionReset, e.to_string())
            })?;

            parser.feed(&raw_chunk);

            while let FrameParseResult::Complete(frame_bytes) = parser.next_frame() {
                let frame = parse_frame_bytes(frame_bytes.clone());

                if frame_complete(&frame, opts.format) {
                    is_complete = true;
                }

                // Cache raw upstream wire bytes (pre-restore), matching Go `captured.Write(frame.Bytes())`.
                cache_accumulator.extend_from_slice(&frame_bytes);

                yield client_frame_bytes(&frame, &opts);
            }
        }

        if let Some(trailing) = parser.drain_remaining() {
            cache_accumulator.extend_from_slice(&trailing);
            yield trailing;
        }

        if is_complete && !cache_accumulator.is_empty() {
            let store_clone = store.clone();
            let entry = Entry {
                key: opts.cache_key.clone(),
                raw_sse: cache_accumulator,
                model: opts.model.clone(),
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            };

            std::thread::spawn(move || {
                if let Err(err) = store_clone.put(entry) {
                    error!(error = %err, "cache put failed");
                } else {
                    debug!("cache stored after stream completion");
                }
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Store;
    use futures_util::stream;
    use std::time::Duration;

    async fn collect_pipeline(
        upstream: impl Stream<Item = Result<Bytes, &'static str>> + Send + Unpin + 'static,
        store: Store,
        opts: PipelineOptions,
    ) -> Vec<Bytes> {
        let mut pipeline = create_processing_pipeline(upstream, store, opts);
        let mut out = Vec::new();
        while let Some(chunk) = pipeline.next().await {
            out.push(chunk.expect("pipeline chunk"));
        }
        out
    }

    fn test_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("cache.db")).unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn yields_bootstrap_first() {
        let (_dir, store) = test_store();
        let upstream = stream::iter(vec![Ok(Bytes::from("data: [DONE]\n\n"))]);
        let opts = PipelineOptions {
            cache_key: "k1".into(),
            model: "gpt-4".into(),
            format: StreamFormat::OpenAI,
            redaction_map: None,
        };

        let out = collect_pipeline(upstream, store, opts).await;
        assert!(out[0].starts_with(b": kortolabs bootstrap"));
        assert!(out.iter().any(|c| c.windows(6).any(|w| w == b"[DONE]")));
    }

    #[tokio::test]
    async fn caches_complete_openai_stream() {
        let (_dir, store) = test_store();
        let upstream = stream::iter(vec![
            Ok(Bytes::from("data: {\"choices\":[]}\n\n")),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ]);
        let opts = PipelineOptions {
            cache_key: "cache-me".into(),
            model: "gpt-4".into(),
            format: StreamFormat::OpenAI,
            redaction_map: None,
        };

        collect_pipeline(upstream, store.clone(), opts).await;
        std::thread::sleep(Duration::from_millis(50));

        let hit = store.get("cache-me").unwrap().expect("cached entry");
        assert!(hit.raw_sse.windows(6).any(|w| w == b"[DONE]"));
        assert!(hit.raw_sse.windows(7).any(|w| w == b"choices"));
    }

    #[tokio::test]
    async fn truncated_stream_not_cached() {
        let (_dir, store) = test_store();
        let upstream = stream::iter(vec![Ok(Bytes::from("data: partial\n\n"))]);
        let opts = PipelineOptions {
            cache_key: "skip-me".into(),
            model: "gpt-4".into(),
            format: StreamFormat::OpenAI,
            redaction_map: None,
        };

        collect_pipeline(upstream, store.clone(), opts).await;
        std::thread::sleep(Duration::from_millis(50));
        assert!(store.get("skip-me").unwrap().is_none());
    }

    #[tokio::test]
    async fn fragmented_upstream_preserves_frames() {
        let (_dir, store) = test_store();
        let upstream = stream::iter(vec![
            Ok(Bytes::from("data: hel")),
            Ok(Bytes::from("lo\n\ndata: [DONE]\n\n")),
        ]);
        let opts = PipelineOptions {
            cache_key: "frag".into(),
            model: "gpt-4".into(),
            format: StreamFormat::OpenAI,
            redaction_map: None,
        };

        let out = collect_pipeline(upstream, store.clone(), opts).await;
        let body: Vec<u8> = out.into_iter().flat_map(|b| b.to_vec()).collect();
        assert!(body.windows(11).any(|w| w == b"data: hello"));
        std::thread::sleep(Duration::from_millis(50));
        assert!(store.get("frag").unwrap().is_some());
    }

    #[tokio::test]
    async fn restores_redaction_on_client_path() {
        let (_dir, store) = test_store();
        let map = Arc::new(RedactionMap::new());
        map.insert("[REDACTED_SECRET_1]", "secret-value");

        let upstream = stream::iter(vec![
            Ok(Bytes::from(
                "data: {\"content\":\"[REDACTED_SECRET_1]\"}\n\n",
            )),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ]);
        let opts = PipelineOptions {
            cache_key: "restore".into(),
            model: "gpt-4".into(),
            format: StreamFormat::OpenAI,
            redaction_map: Some(map),
        };

        let out = collect_pipeline(upstream, store.clone(), opts).await;
        let client_body: Vec<u8> = out.into_iter().flat_map(|b| b.to_vec()).collect();
        assert!(client_body.windows(12).any(|w| w == b"secret-value"));

        std::thread::sleep(Duration::from_millis(50));
        let cached = store.get("restore").unwrap().expect("cached");
        // Cache retains upstream wire bytes (still redacted).
        assert!(cached
            .raw_sse
            .windows(19)
            .any(|w| w == b"[REDACTED_SECRET_1]"));
    }

    #[tokio::test]
    async fn dropped_pipeline_skips_cache_write() {
        let (_dir, store) = test_store();
        let upstream = stream::iter(vec![
            Ok::<Bytes, &'static str>(Bytes::from("data: {\"x\":1}\n\n")),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ]);
        let opts = PipelineOptions {
            cache_key: "dropped".into(),
            model: "gpt-4".into(),
            format: StreamFormat::OpenAI,
            redaction_map: None,
        };

        let mut pipeline = create_processing_pipeline(upstream, store.clone(), opts);
        let _ = pipeline.next().await; // bootstrap only
        drop(pipeline);

        std::thread::sleep(Duration::from_millis(50));
        assert!(store.get("dropped").unwrap().is_none());
    }
}
