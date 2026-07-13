# Kotro Next Steps — Prioritized Task List

*Companion to `docs/review/2026-07-strategic-review.md`. Context: Go was the Phase 1 reference implementation (chosen for strong SSE handling); Rust is the intended end state. This list sequences the remaining Go→Rust convergence alongside the trust/legal fixes and the real-semantic-cache work.*

## P0 — This week (blocking, near-zero effort)

- [ ] Add root `LICENSE` file (MIT). `rust/Cargo.toml` already declares `license = "MIT"` — the decision is made, the file is just missing. Nothing in the repo is legally usable by anyone until this exists.
- [ ] Fix README claims to match current behavior: "semantic cache" is exact-match SHA-256 today (Go and current Rust cache path); "MoE routing" is a regex keyword matcher. Rename or explicitly scope both until P2 ships.
- [ ] Reframe the 99.3% benchmark: separate "Kotro-attributable savings" from "upstream DeepSeek prefix-cache savings" — the published numbers show a local proxy *miss* followed by an upstream *hit*.

## P1 — Verify Rust independently, then freeze Go

Revised framing (see conversation log): "match Go's test count" is a proxy metric, not the goal. Go's 74 `#[test]`-equivalents split into two buckets — Go-specific plumbing (`io.Pipe` watchdog behavior, bbolt quirks) that has no meaningful Rust equivalent and shouldn't be ported, and behavioral invariants (tenant isolation, redaction correctness, protocol parsing) that must hold regardless of language and are currently *proven* on Go but only *assumed* to carry over to Rust. The work below targets the second bucket specifically, not the raw count.

- [ ] Run `cargo-llvm-cov` (or similar) on the Rust crate to find real coverage gaps, instead of diffing against Go's test count.
- [ ] Prioritize four security/reliability-critical areas regardless of Go's count: tenant/scope isolation (Rust equivalents of `TestCacheIsolation_TenantSeparation` / `TestAnthropicCacheIsolation_TenantSeparation` — this is what the threat model doc's isolation claims rest on), redaction correctness, SSE frame parsing edge cases (parity against Go's `stream_test.go` vectors per `docs/RUST-ARCHITECTURE.md`), and the cancel-storm leak audit. Everything else (encoding edge cases, eviction timing) is lower priority, and some may be moot since Rust's type system eliminates bug classes (nil pointers, unchecked type assertions) Go needed tests to guard against.
- [ ] Wire `make rust-cancel-audit` into CI or a scheduled workflow. It exists in the Makefile (`benchmarks/run_rust_audit.sh`) but `.github/workflows/ci.yml` only runs `cargo test` — this is the single highest-consequence gap in this list, since an undetected thread/RSS leak ships straight to users' machines.
- [ ] Verify distribution parity: confirm npm, Homebrew, Docker, and the VS Code extension are all shipping the Rust binary (a commit indicates this switched already) — audit for any channel silently still on Go.
- [ ] Run `make eval-suite` against both binaries and diff results once, as a sanity check that Rust matches Go's behavior on cache hit rate, redaction correctness, and compression ratio — a one-time confirmation, not an ongoing parity requirement.
- [ ] Declare Go frozen once the four critical areas above are independently verified in Rust: tag a final Go release, mark `internal/` as reference-only in the README, shrink Go's CI job to compile + smoke test (not the full suite), and route all new feature work through Rust exclusively from that point.

## P2 — Make the semantic cache real

- [x] Wire `candle-core` / `candle-nn` / `candle-transformers` / `hf-hub` into `SemanticEncoder::embed()` in `cache/vector.rs`, replacing the byte-sum stub with real `all-MiniLM-L6-v2` inference. Done in `69d0035`; compiles and runs against the pinned `0.11.0` candle versions (verified locally — `cargo build` clean).
- [x] Add lazy-download-with-offline-fallback: fetch weights via `hf-hub` on first run; if unavailable, fall back to exact-match cache rather than failing startup. Implemented in `SemanticEncoder::new()` — a load failure logs a warning and degrades to a disabled encoder rather than panicking; confirmed the happy path works (model downloads, loads, and runs) via local `cargo test`.
- [x] Replace the current stub test with real accuracy tests: paraphrase pairs that should hit, unrelated prompts that shouldn't, at a tuned cosine threshold. Done — `semantic_similarity_reflects_paraphrase_vs_unrelated` and `vector_index_lookup_uses_encoder_output` both pass locally (`cb49700` recalibrated one threshold after the first real run showed mean-pooled MiniLM's actual anisotropy baseline; see commit message for the reasoning). All 3 tests in `cache::vector` pass as of the latest local run.
- [x] Benchmark embedding latency overhead and publish it next to cache-hit-rate numbers. Measured locally via `cargo run --release --example bench_embedding` (`rust/kotro-proxy/examples/bench_embedding.rs`, added in `bdf75c5`):

  | Prompt shape | mean | p50 | p95 | p99 |
  |---|---|---|---|---|
  | short (~8 words) | 25.9ms | 25.9ms | 26.6ms | 27.7ms |
  | medium (~46 words, inline code) | 26.7ms | 26.6ms | 27.1ms | 27.5ms |
  | long (~257 words, file-content turn) | 27.3ms | 27.3ms | 27.8ms | 28.2ms |

  One-time model load (warm HF cache, weights already downloaded): 151.8ms — happens once at proxy startup, not per request.

  **Interpretation:** latency is flat across prompt sizes (~26-28ms) — dominated by fixed model compute, not input length, until you approach the 512-token truncation ceiling. In absolute terms this is imperceptible to a human waiting on their IDE, and small relative to typical upstream provider round-trip time (hundreds of ms to seconds). Two things worth being explicit about rather than glossing over: (1) `embed()` runs on *every* request when the vector cache is enabled, including on exact-match and vector-cache *misses* — so this ~26ms is a flat tax paid even when it doesn't produce a hit, not just an amortized cost that comes with a savings payoff. It's ~13x the default `KOTRO_CACHE_HIT_DELAY_MS` (2ms) exact-match replay pacing. (2) ~~`embed()` currently blocks its tokio worker thread synchronously~~ **Fixed**: the two call sites in `router/handlers.rs` now go through a `embed_off_thread()` helper that runs the embedding call via `tokio::task::spawn_blocking`, so it no longer competes with async I/O work on the same worker threads. Needs a local `cargo build && cargo test` pass to confirm — same environment constraint as the rest of P2.

## P3 — Trust and launch readiness

- [ ] Add `CONTRIBUTING.md`, GitHub issue templates, `CODE_OF_CONDUCT.md` — none currently exist in `.github/`.
- [ ] Make `benchmarks/eval-suite/RESULTS.md` a living artifact re-run and committed on every release.
- [ ] Add a README comparison table vs. LiteLLM / Portkey stating plainly who should use which — narrow the pitch to "single-binary, zero-dependency, local-first proxy for coding agents."
- [ ] Design-partner outreach + Show HN launch, per the existing `docs/roadmap/90-DAY-ROADMAP.md` — sequence after P0–P2 since the launch post will be read against source code.

## P4 — Growth and ecosystem positioning (after P0–P3)

- [ ] Position Kotro explicitly as an **MCP-aware local proxy**, not a generic LLM gateway. The context compressor already touches MCP tool schemas — lean into this in the README/docs and in any launch content, since MCP is the fastest-growing integration surface in the coding-agent space right now and a more specific, timelier claim than "AI proxy."
- [ ] Build a real **extension/plugin surface** so other teams can build on top of Kotro, not just run it. Concretely: a trait-based interface for custom cache backends and custom redaction rules, or a WASM plugin surface for compliance rules; and publish the core logic as a separate library crate (e.g. `kotro-core` on crates.io) that can be embedded, not just invoked as a binary. This is the actual unlock for "company builds their own product on top of Kotro" rather than "company runs the binary."
- [ ] Add **supply-chain trust signals**: signed releases (cosign/sigstore), an SBOM per release, reproducible builds. Cheap relative to the trust it buys — security teams evaluating a new dependency that proxies API keys check for this by default.
- [ ] Build a **content flywheel around the pain, not the tool**: problem-aware posts targeting real search intent ("why is my Cursor bill so high," "reduce Claude Code API costs") and an honest "Kotro vs LiteLLM vs Portkey" comparison (including where Kotro loses). This converts to organic stars far better than a single launch spike.
- [ ] Publish a short **technical writeup** on the combined approach (local semantic cache + AST-aware context pruning + agent-loop circuit breaking, coordinated with upstream prefix caching) once the semantic cache is real — `docs/RUST-ARCHITECTURE.md` already frames the Rust port as "suitable for... arXiv publication." A clean writeup is citable, external evidence of the system's design, independent of GitHub metrics.
- [ ] Prioritize **design partners with a measurable, quotable result** over raw star count — a few teams who can say "this cut our bill by X%" is stronger, more durable proof than stars with no usage behind them, and compounds into the next round of adoption.
