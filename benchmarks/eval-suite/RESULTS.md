# Kotro Proxy — Eval Suite Results

> **Last updated:** 2026-08-02 (Rust `v0.6.2` launch hygiene).  
> DeepSeek API scenarios below are a **historical baseline** from the Go reference era. Re-run live provider numbers with `make eval-suite` (requires `DEEPSEEK_API_KEY`). Correctness today is gated by `cargo test` + Escape Lab, not by replaying this DeepSeek table on every commit.

---

## Summary

| Metric | Value |
|--------|-------|
| Upstream token reduction (DeepSeek, 3-turn historical) | **99.3%** (provider prefix cache; see caveat below) |
| Local proxy cache hits (that historical benchmark) | 0/3 turns — each turn had new content |
| Redaction unit tests | 17/17 (`guardrail/redactor.rs`) |
| Injection unit tests | 16/16 (`guardrail/injection.rs`) |
| Budget unit tests | 11/11 (`budget/mod.rs`) |
| Rust `kotro-proxy` lib suite | **336 passed, 1 ignored, 0 failed** (2026-08-02) |
| Companion crates | `kotro-types` 19 · `kotro-schema` 18 · `kotro-core` 24 |
| Escape Lab corpus | **15** scenarios valid; **14/14** HTTP-measured match declared behaviour |

**Read the 99.3% number carefully.** In that historical DeepSeek run every turn had new content, so Kotro's own local cache missed every turn — each request was forwarded upstream. The 99.3% reduction is DeepSeek's server-side prefix cache on Turns 2 and 3; Kotro's contribution is keeping the request shape stable so upstream prefix caching can fire. Kotro's local cache is a second, independent savings layer on genuinely repeated prompts (retries, shared fixtures). That scenario is demonstrated by `make demo-savings` (~68%), not by the DeepSeek table below.

**Active implementation:** Rust (`kotro-proxy` **0.6.2**). Go under `internal/` is frozen at tag `v0.1.0-go` (CI compiles it; new feature work is Rust-only).

---

## Correctness gates (current)

These are what a reader should re-run before trusting launch claims. They do not require a provider API key.

### Rust unit suite

```bash
cd rust && cargo test -p kotro-proxy --lib
# expected: 336 passed; 0 failed; 1 ignored
```

The single ignored test is a timing gate (`corpus::in_process_admitted_schema_policy_under_5ms_p95`); stable numbers live in the Criterion bench `mcp_hot_path`.

| Area | Tests (lib) | What they cover |
|------|-------------|-----------------|
| `guardrail/` (injection, redaction, loop detector, …) | 42 | Prompt-injection patterns, PII/secret redaction map, agent-loop circuit breaker |
| `mcp/` | 39 | Wrap plane, schema/pin/protect, TaskEnvelope / task gate, list cache, routing |
| `cache/` | 45 | Exact-match store, tool cache, vector/semantic encoder paths |
| `router/` | 63 | Scope isolation, governance / kill switch / mode dial, handlers, approvals |
| `budget/` | 11 | Per-scope token budget, warn + hard block |
| `optimizer/` | 15 | Reasoning-token caps (Anthropic / OpenAI families) |
| `posture/` / `flight_recorder/` / `policy/` | 16 / 16 / 15 | Runtime posture, append-only tape, policy surface |
| **`kotro-proxy` total** | **337 listed · 336 run** | 1 ignored timing gate |

Companion crates (not counted above): `kotro-types` (mode dial types), `kotro-schema` (telemetry / admitted schema), `kotro-core` (embeddable core).

### Escape Lab (adversarial regression matrix)

```bash
python3 scripts/escape-lab.py --validate          # 15 scenarios, schema-valid
bash scripts/run-escape-lab-matrix.sh             # all env groups → merged matrix
```

Published matrix: [`docs/security/ESCAPE-LAB-MATRIX.md`](../../docs/security/ESCAPE-LAB-MATRIX.md).

| Signal | Result |
|--------|--------|
| Corpus size | 15 scenarios (EL-01…EL-15) |
| HTTP-measured | 14/14 match declared behaviour (EL-05 is CLI-only / MCP rug-pull) |
| Covered (prevent / transform / detect) | 9/14 |
| Mode dial | EL-12 `audit`→detect · EL-13 `disabled`→none |
| Kill switch vs mode | EL-14 / EL-15 kill switch still **prevent** under `disabled` / `audit` |

Honest gaps called out in the matrix (not hidden): encoded secret exfil (EL-08), unauthorized egress (EL-09), cross-session memory writes (EL-11). Detail in [`docs/security/THREAT-MODEL.md`](../../docs/security/THREAT-MODEL.md).

### Product surface added after the Go baseline

| Capability | How it's proven |
|------------|-----------------|
| `KOTRO_MODE=enforce\|audit\|disabled` | Unit + Escape Lab EL-12…EL-15; `x-kotro-mode` stamped on responses |
| Kill switch outranks mode (LLM + MCP) | EL-06 / EL-14 / EL-15; governance runs halt before `evaluates()` |
| MCP wrap (stdio / Streamable HTTP) + TaskEnvelope | `mcp/` unit suite; `kotro-proxy mcp-wrap` CLI |
| Injection warn vs block | EL-01 detect · EL-02 prevent; HTTP **400** when blocked |
| Session token budget | EL-07 prevent; HTTP **429** when blocked |
| Runtime posture | `/api/runtime-posture` + posture unit tests |

---

## Methodology (historical DeepSeek / Qwen baseline)

**Setup (original run):** Kotro **Go** reference implementation (now frozen at `v0.1.0-go`) in front of DeepSeek API (`deepseek-chat`) and Alibaba DashScope (`qwen-plus`). 3-turn coding agent conversation with a ~2000-token system context (200-line Go file). Cache strategies tested: `FullDigest` and `WindowN` (last 4 messages).

**Server cache hits:** DeepSeek implements KV-cache prefix caching server-side (`prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`). Kotro preserves request prefix shape so that cache can match.

**Local proxy cache hits:** A local HIT means Kotro replayed the full SSE response from its own store with zero upstream round-trip. This historical benchmark had novel content each turn → no local hits.

A one-time Go-vs-Rust `make eval-suite` diff against a live provider is still desirable (see `docs/roadmap/next-steps.md` P1) but is **not** required to trust the security gates above.

---

## Scenario A: DeepSeek — FullDigest Strategy

| Turn | Prompt Tokens | Server Cache Hits | Server Cache Misses | Local Proxy |
|------|--------------|-------------------|---------------------|-------------|
| 1 | 2,042 | 1,920 | 122 | 🔴 MISS |
| 2 | 2,061 | 2,048 | 13 | 🔴 MISS |
| 3 | 2,079 | 2,048 | 31 | 🔴 MISS |

Turn 1 already shows 1,920/2,042 (94%) server cache hits — the static code context matches a prior session's prefix in DeepSeek's KV cache. Turns 2 and 3 add only new turn content (13 and 31 miss-tokens respectively). Total billed across 3 turns: ~166 tokens out of 6,182 sent — **97.3% server-side reduction** in this run.

---

## Scenario B: DeepSeek — WindowN Strategy

| Turn | Prompt Tokens | Server Cache Hits | Server Cache Misses | Local Proxy |
|------|--------------|-------------------|---------------------|-------------|
| 1 | 2,042 | 1,920 | 122 | 🔴 MISS |
| 2 | 2,061 | 2,048 | 13 | 🔴 MISS |
| 3 | 2,079 | 2,048 | 31 | 🔴 MISS |

Identical to FullDigest for this 3-turn window (WindowN with size=4 covers the full history here). WindowN produces smaller cache keys and is the recommended strategy for long coding sessions where full-digest keys grow unbounded.

---

## Scenarios C & D: Qwen (DashScope)

Qwen's OpenAI-compatible endpoint does not expose `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`. All token counts returned as 0. Qwen does implement KV-cache prefix caching internally, but it is not observable through this API.

---

## What's Not Yet Measured

| Item | Notes |
|------|-------|
| Live `make eval-suite` re-run on the Rust binary | Historical table is Go-era. One-time Go-vs-Rust provider diff is deferred (P1 in `next-steps.md`), not a launch blocker. |
| Local cache hit rate on repeated-prompt workload | Use `make demo-savings` for the honest local-cache story (~68%). |
| Context compression ratio on real sessions | Compressor tests verify correctness; ratio on long coding sessions not published. |
| Semantic cache (MiniLM) hit rate in the eval harness | Encoder latency published via `bench_embedding`; paraphrase hit-rate fixture not yet in `eval-suite/`. |
| Line coverage (`cargo-llvm-cov`) | Deferred systematic coverage pass (P1). |
| Auto-refresh of this file on every release | Deferred release-process work (P3) — wire `make eval-suite` / Escape Lab into `release.yml` later. |
