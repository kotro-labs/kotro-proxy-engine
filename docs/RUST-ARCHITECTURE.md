# Korto Proxy Engine — Rust Architecture Map (Phase 2)

This document translates the **verified Go Phase 1 semantics** into a zero-cost Rust implementation suitable for cross-compilation, arXiv publication, and production deployment without a garbage collector.

## Go → Rust semantic contract

The Go reference on `main` is the **source of truth**. Every Rust module must preserve these invariants (validated by 35+ Go tests and the cancel-storm pprof audit):

| Invariant | Go implementation | Rust target |
|-----------|-------------------|-------------|
| SSE frame boundaries | `internal/sse` blank-line parser | `sse::Frame` + `FrameReader` |
| Cache key | SHA-256(system ∥ user ∥ model ∥ provider) | `cache::semantic::key_for_request` |
| TTL wire format | 8-byte BE `i64` UnixNano + JSON | `cache::encoding` (byte-identical) |
| Legacy cache entries | Leading `{` = no prefix | Same rule in `decode_stored_value` |
| Cache miss path | `io.Pipe` + watchdog on `ctx.Done()` | `async_stream` tee; drop = cleanup (RAII) |
| Cache hit path | Per-frame `ctx` check + bootstrap | `replay_cached` async stream |
| HTTP/2 TTFB | SSE bootstrap comment + flush | `bootstrap::prime_sse` + `hyper` flush |
| Provider routes | `/v1/chat/completions`, `/v1/messages` | `router::openai`, `router::anthropic` |

---

## Recommended initiation order

**Start with the storage layer (`cache::encoding` + `redb`), not axum routing.**

| Order | Module | Rationale |
|-------|--------|-----------|
| 1 | `cache/encoding` + `cache/store` | Pure logic, byte-identical to Go; unit-testable without async |
| 2 | `sse/frame` | Frame parser parity tests against Go `stream_test.go` vectors |
| 3 | `proxy/pipeline` | `bytes::Bytes` stream transform + redaction hook |
| 4 | `router` + `bootstrap` | axum handlers, HTTP/2 flush, auth header forward |
| 5 | `eviction` + `metrics` | Background tokio task, `tracing` + optional `pprof` |

Routing last avoids debugging HTTP/2 flush and redb TTL simultaneously. Storage + SSE are the **contract spine**.

---

## Stack selection

```
┌─────────────────────────────────────────────────────────┐
│  korto-proxy (single binary)                            │
├─────────────────────────────────────────────────────────┤
│  axum 0.7+          HTTP router, extractor, middleware│
│  tokio              async runtime, timers, signals      │
│  hyper 1.x          upstream client, frame control    │
│  tower              middleware (timeout, trace)         │
├─────────────────────────────────────────────────────────┤
│  bytes              zero-copy SSE chunk windows         │
│  async-stream       lazy stream combinators           │
│  futures-util       StreamExt, select!, forward         │
├─────────────────────────────────────────────────────────┤
│  redb               embedded MVCC B+ tree (no C deps) │
│  serde_json         cache entry JSON (same schema)    │
│  sha2               semantic cache keys                 │
│  regex              guardrail patterns                │
├─────────────────────────────────────────────────────────┤
│  tracing            structured JSON logs (≈ slog)     │
│  tracing-subscriber                                         │
└─────────────────────────────────────────────────────────┘
```

**Why `redb` over `sled`:** `redb` uses append-only MVCC with explicit read/write transactions—closest mental model to bbolt's `View`/`Update`. Pure Rust, `no_std`-friendly core, no libc requirement for cross-compilation.

---

## Crate layout

```
rust/
├── Cargo.toml                 # workspace
└── korto-proxy/
    ├── Cargo.toml
    └── src/
        ├── main.rs            # tokio::main, signal, graceful shutdown
        ├── lib.rs
        ├── config.rs          # KORTO_* env → Config struct
        ├── cache/
        │   ├── mod.rs
        │   ├── encoding.rs    # 8-byte TTL prefix (Phase 2.1) ✓
        │   ├── semantic.rs    # SHA-256 keys
        │   ├── store.rs       # redb wrapper: get/put/delete/sweep
        │   └── eviction.rs    # tokio::interval sweep task
        ├── sse/
        │   ├── mod.rs
        │   ├── frame.rs       # Frame, is_openai_done, is_anthropic_complete
        │   └── reader.rs    # incremental frame parser
        ├── guardrail/
        │   └── redactor.rs    # placeholder map, per-request registry
        ├── compressor/
        │   └── context.rs     # block-hash dedup across turns
        ├── proxy/
        │   ├── mod.rs
        │   ├── bootstrap.rs   # ": kortolabs bootstrap stream\n\n"
        │   ├── pipeline.rs    # redact_and_cache_stream
        │   ├── openai.rs      # chat/completions handler
        │   └── anthropic.rs   # messages handler
        └── router.rs          # axum::Router assembly
```

---

## Storage: redb schema + TTL wire format

### Table definition

```rust
// redb 2.x
const CACHE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("sse_cache");
```

- **Key:** semantic cache key (hex SHA-256 string)
- **Value:** `encode_stored_value(expires_at_nano, json_entry)`

### Wire layout (byte-identical to Go)

```
┌──────────────────┬──────────────────────────────────────┐
│ 8 bytes          │ N bytes                              │
│ BE u64           │ JSON {"Key","RawSSE","Model",...}    │
│ expires_at_nano  │ same schema as Go cache.Entry        │
└──────────────────┴──────────────────────────────────────┘

Legacy: value[0] == b'{' → no TTL prefix (Go migration compat)
```

### Read path

```rust
pub fn get(&self, key: &str) -> Result<Option<CacheEntry>> {
    let read = self.db.begin_read()?;
    let table = read.open_table(CACHE_TABLE)?;
    let Some(raw) = table.get(key)? else { return Ok(None) };
    let (payload, expired) = decode_stored_value(raw.value(), now_nano());
    if expired {
        let key = key.to_owned();
        tokio::spawn(async move { let _ = self.delete(&key).await; });
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(payload)?))
}
```

### Eviction worker

```rust
pub async fn run_eviction_worker(self: Arc<Self>, mut shutdown: CancellationToken) {
    let mut ticker = tokio::time::interval(self.eviction_interval);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = ticker.tick() => {
                let n = self.sweep_expired()?;
                if n > 0 { tracing::info!(deleted = n, "cache eviction sweep"); }
            }
        }
    }
}
```

**Freelist note (document in whitepaper):** redb, like bbolt, recycles pages internally; file size may not shrink after delete.

---

## Concurrency: Go watchdog vs Rust RAII

### Go (Phase 1)

```
Client disconnect
    → context.Cancel
    → watchdog: pw.CloseWithError + upstream.Close()
    → unblocks blocked Read/Write in pipe goroutine
```

Cooperative cancellation required because goroutines cannot be preemptively killed.

### Rust (Phase 2)

```
Client disconnect
    → axum drops response Future
    → upstream hyper Body Stream dropped
    → TCP half-close propagates
    → all owned buffers dropped at end of scope
```

No watchdog sidecar needed for the **miss path**. Implement `Drop` on `InterceptGuard` only for explicit metrics/logging.

```rust
struct InterceptGuard {
    key: String,
    started: Instant,
}

impl Drop for InterceptGuard {
    fn drop(&mut self) {
        tracing::debug!(key = %self.key, elapsed = ?self.started.elapsed(), "stream ended");
    }
}
```

**Cache-hit path** still needs explicit cancellation checks inside the replay loop (same as Go `replayCached`) because the handler owns the full byte stream before the client disconnects.

---

## Streaming pipeline

```rust
use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;

pub type SseStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProxyError>> + Send>>;

pub fn intercept_stream(
    upstream: SseStream,
    ctx: RequestContext,
    cache: Arc<CacheStore>,
) -> SseStream {
    Box::pin(async_stream::try_stream! {
        yield bootstrap::comment();           // ": kortolabs bootstrap stream\n\n"

        let mut captured = Vec::new();
        tokio::pin!(upstream);

        while let Some(frame) = upstream.next().await {
            let frame = frame?;
            if ctx.redaction_map.len() > 0 {
                frame = guardrail::restore_frame(frame, &ctx.redaction_map)?;
            }
            captured.extend_from_slice(&frame.to_bytes());
            yield frame.to_bytes();
            if frame.is_complete(ctx.format) {
                cache.put_complete(ctx.cache_key, captured, ctx.model).await?;
                break;
            }
        }
    })
}
```

When the client disconnects, the outer `yield` loop stops polling → upstream stream dropped → no leak.

---

## axum routing map

| Route | Handler | Notes |
|-------|---------|-------|
| `POST /v1/chat/completions` | `openai::chat_completions` | OpenAI SSE format |
| `POST /v1/messages` | `anthropic::messages` | `event:` + `data:` format |
| `/v1/*` | `passthrough` | hyper reverse proxy |
| `GET /healthz` | `health` | JSON status |
| `GET /debug/pprof/*` | `pprof` (feature = "pprof") | Leak audits |

### Bootstrap flush (HTTP/2 TTFB)

```rust
async fn prime_sse<B>(body: B) -> Result<(HeaderMap, SseStream), ProxyError>
where
    B: Body + Send + 'static,
{
    let mut headers = sse_headers();
    headers.insert("x-accel-buffering", "no");
    let bootstrap = Bytes::from_static(b": kortolabs bootstrap stream\n\n");
    let stream = once(async move { Ok(bootstrap) }).chain(body);
    Ok((headers, Box::pin(stream)))
}
```

Use `hyper::body::Body::data_frame` flush semantics via axum `BodyDataStream` where available.

---

## Configuration parity

| Env var | Go default | Rust default |
|---------|------------|--------------|
| `KORTO_LISTEN_ADDR` | `:8080` | `0.0.0.0:8080` |
| `KORTO_UPSTREAM_URL` | `http://127.0.0.1:9000` | same |
| `KORTO_CACHE_TTL` | `24h` | `Duration::from_secs(86400)` |
| `KORTO_EVICTION_INTERVAL` | `10m` | `Duration::from_secs(600)` |
| `KORTO_ENABLE_CACHE` | `true` | `true` |
| `KORTO_ENABLE_PPROF` | `false` | `false` |

---

## Test parity matrix

| Go test | Rust test target |
|---------|------------------|
| `encoding_test.go` | `cache::encoding::tests` |
| `ttl_test.go` | `cache::store::tests` + `tempfile` redb |
| `stream_test.go` | `sse::frame::tests` |
| `stream_watchdog_test.go` | **N/A** — replaced by drop + `tokio::test` cancel |
| `sse_bootstrap_test.go` | `proxy::bootstrap::tests` |
| `handler_cancel_test.go` | `router::integration` + `reqwest` cancel |
| `cancel_storm.js` + `run_audit.sh` | Same k6 scripts, point at Rust binary |

---

## Cross-compilation targets

```bash
rustup target add aarch64-unknown-linux-gnu
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

Single static binary (~8–12 MB) with no libc dependency on musl targets.

---

## Whitepaper outline (arXiv draft)

1. **Problem:** Local LLM agent proxies must intercept SSE without buffering; generic reverse proxies fail on TTFB and leak goroutines on cancel.
2. **Go reference:** Asymmetric pipe + watchdog; empirical 0-goroutine delta (k6 + pprof).
3. **Rust translation:** RAII cancellation, `bytes::Bytes` zero-copy framing, redb MVCC cache.
4. **Evaluation:** Latency (TTFB), throughput (k6), memory (heaptrack), cancel-storm goroutine/thread count.
5. **Conclusion:** Compile-time ownership eliminates watchdog complexity while preserving semantic cache fidelity.

---

## Next implementation PR (Rust Phase 2.1)

1. `cache/encoding.rs` — **done in scaffold** (Go parity tests)
2. `cache/store.rs` — redb open/get/put/delete
3. Port Go `stream_test.go` vectors to `sse/frame.rs`

Then vertical slice: OpenAI cache miss → store → cache hit replay.
