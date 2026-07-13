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

- [ ] Wire `candle-core` / `candle-nn` / `candle-transformers` / `hf-hub` (already in `rust/kotro-proxy/Cargo.toml`, currently unused) into `SemanticEncoder::embed()` in `cache/vector.rs`, replacing the byte-sum stub with real `all-MiniLM-L6-v2` inference.
- [ ] Add lazy-download-with-offline-fallback: fetch weights via `hf-hub` on first run; if unavailable, fall back to exact-match cache rather than failing startup — preserves the zero-config, single-binary promise.
- [ ] Replace the current stub test (`test_vector_index_similarity`, which only checks identical strings) with real accuracy tests: paraphrase pairs that should hit, unrelated prompts that shouldn't, at a tuned cosine threshold.
- [ ] Benchmark embedding latency overhead and publish it next to cache-hit-rate numbers — must stay low enough that it doesn't erode the savings it creates.

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
