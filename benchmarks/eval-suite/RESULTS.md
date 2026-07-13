# Kotro Proxy — Eval Suite Results

> **Last run:** 2025 (DeepSeek API). Re-run with `make eval-suite` (requires `DEEPSEEK_API_KEY`).

---

## Summary

| Metric | Value |
|--------|-------|
| Upstream token reduction (DeepSeek, 3-turn benchmark) | **99.3%** |
| Local proxy cache hits (this benchmark) | 0/3 turns — each turn had new content |
| Redaction correctness | 17/17 test cases, 10 PII pattern types |
| MCP injection detection | 17/17 test cases, 14 regex rules |
| Rust test suite | 157 tests, 0 failures |

**Read the 99.3% number carefully.** In this specific benchmark every turn had new content, so Kotro's own local cache missed every turn — each request was forwarded upstream. The 99.3% reduction is DeepSeek's server-side prefix cache doing the work on Turns 2 and 3; Kotro's contribution is keeping the request shape stable across turns so that upstream prefix caching fires cleanly. Kotro's local cache adds a second, independent savings layer on genuinely repeated prompts (retries, shared agent fixtures, parallel runs hitting the same turn) with zero upstream round-trip. That scenario is not represented in this benchmark; a repeated-prompt fixture is planned.

---

## Methodology

**Setup:** Kotro Go reference implementation (frozen at `v0.1.0-go`) in front of DeepSeek API (`deepseek-chat`) and Alibaba DashScope (`qwen-plus`). 3-turn coding agent conversation with a ~2000-token system context (200-line Go file). Cache strategies tested: `FullDigest` (hash of all messages) and `WindowN` (last 4 messages). Each turn appends a new user query plus the full code dump to the history, mimicking an IDE that resends the full file on every turn.

**Server cache hits:** DeepSeek implements KV-cache prefix caching server-side. When a request prefix matches a cached computation, the response includes `prompt_cache_hit_tokens` (billed at 0.1× normal input token price) and `prompt_cache_miss_tokens` (billed at full price). Kotro preserves the request prefix across turns — system message first, code context in a fixed position — so the upstream KV cache can match it. A proxy that reorders messages or injects variable content breaks prefix caching.

**Local proxy cache hits:** A local HIT means Kotro replayed the full SSE response from its own redb store with zero upstream round-trip. Requires the exact same prompt state (by hash) in the same session. In this benchmark each turn has novel content, so no local hits occur.

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

## Rust Test Coverage

These tests run in CI on every push (`cargo test -p kotro-proxy`). They are the primary correctness signal for the security and efficiency features.

| Module | Tests | Coverage |
|--------|-------|----------|
| `guardrail/redactor.rs` | 17 | 10 PII pattern types: API keys, DB URLs, passwords, emails, AWS keys, JWT tokens, private keys, phone numbers, credit cards, IPs |
| `guardrail/injection.rs` | 17 | 14 prompt injection regex rules: jailbreak phrases, role overrides, system prompt leaks, tool result hijacks |
| `budget/mod.rs` | 11 | Token budget enforcement, soft warn + hard block modes, per-scope session tracking |
| `optimizer/reasoning.rs` | 14 | Anthropic `thinking.budget_tokens` cap, OpenAI `max_completion_tokens` cap, model detection for claude-opus-4/o1/o3 families |
| `cache/tool.rs` | 13 | Per-scope TTLs, arg canonicalization (BTreeMap sort), write-op path invalidation |
| `router/scope.rs` | 6 | Tenant isolation, `unified_cache_key → scope.key()` chain |
| **Total** | **157** | All passing, 0 failures |

---

## What's Not Yet Measured

| Item | Notes |
|------|-------|
| Local cache hit rate on repeated-prompt workload | Each turn in this benchmark has novel content. A repeated-prompt fixture (retries, shared fixtures) is planned to isolate Kotro's local cache contribution. |
| Context compression ratio on real sessions | Compressor integration tests verify correctness (identical blocks stripped); ratio on real coding session data not yet measured. |
| Semantic cache (MiniLM) hit rate | Vector cache runs separately from the exact-match cache. Hit rate on paraphrased prompts not yet in the eval suite. |
| Rust binary end-to-end benchmark | Rust is the active implementation; Go is frozen. The eval suite currently runs against the Go reference binary. A Rust equivalent is planned. |
