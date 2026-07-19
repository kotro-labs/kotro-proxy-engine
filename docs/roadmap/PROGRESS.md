# Kotro — Feature Progress Tracker

> **Read this at the start of every session.** Tracks what's built, what's next, and the launch checklist.
> Update the checkbox and add a one-line note when anything ships.

---

## ✅ Completed Features

| Feature | File(s) | Notes |
|---------|---------|-------|
| MCP prompt injection scanner (request-side) | `guardrail/injection.rs` | 14 regex patterns, warn-by-default, block on `KOTRO_INJECTION_BLOCK=true`. 17 tests. |
| Per-session cost budget enforcement | `budget/mod.rs` | `KOTRO_SESSION_TOKEN_BUDGET`, soft warn + hard block mode. 11 tests. |
| Agent loop circuit breaker | `guardrail/loop_detector.rs` | Detects 3+ identical tool calls in window. `X-Kotro-Circuit-Open` header. |
| Intelligent model router | `router/classifier.rs` | `PromptComplexity` tiers: Nano/Micro/Standard/Complex. Routes by heuristics. |
| Semantic cache (MiniLM) | `cache/vector.rs` | `all-MiniLM-L6-v2` via candle. Cosine threshold 0.94. `KOTRO_ENABLE_VECTOR_CACHE`. |
| SHA-256 exact-match cache | `cache/store.rs` | redb-backed. `WindowN` strategy (last 4 messages). TTL 24h, eviction 600s. |
| PII / secret redaction | `guardrail/redactor.rs` | 10 pattern types including DB URLs, passwords, emails. Restores in streamed response. |
| Reasoning model budget controller | `optimizer/reasoning.rs` | Caps `thinking.budget_tokens` (Anthropic) / `max_completion_tokens` (OpenAI). `KOTRO_MAX_THINKING_TOKENS`, `KOTRO_REASONING_BLOCK`. 14 tests. |
| MCP tool call response cache | `cache/tool.rs` | In-memory, per-scope, per-category TTL (read=30s/status=5m/search=1h). Write-op path invalidation. `KOTRO_ENABLE_TOOL_CACHE`. 13 tests. |
| Go freeze | `internal/`, `.github/workflows/ci.yml`, `README.md` | Tagged `v0.1.0-go`. CI reduced to `go build ./...` only. README marks `internal/` as frozen reference. |
| kotro-core library crate | `rust/kotro-core/` | Embeddable crate, foundation for WASM plugin surface. |
| Supply-chain signing (SBOM + cosign) | `.github/workflows/release.yml` | Keyless OIDC signing via Sigstore. SPDX SBOM generated on release. |
| CI cancel-storm leak audit | `.github/workflows/cancel-audit.yml` | k6 load test, goroutine leak detection. Separate non-blocking workflow. |
| Redaction correctness tests | `guardrail/redactor.rs` | 17 tests. Fixed 6 missing patterns vs. Go reference. |
| Tenant/scope isolation tests | `router/scope.rs` | 6 tests. Full `unified_cache_key → scope.key()` chain verified. |
| MIT license + README fixes | `LICENSE`, `README.md` | License added, benchmark reframed, semantic cache renamed accurately. |
| CONTRIBUTING, CoC, issue templates | repo root + `.github/` | Bug/feature templates ask Rust vs. Go. Security reports → private advisory. |
| README comparison table | `README.md` | Kotro vs. LiteLLM vs. Portkey — narrower, defensible pitch. |
| GitHub storefront | github.com/kotro-labs | Description, topics, website set. |
| Demo savings script | `scripts/demo-savings.sh` | `make demo-savings`. Outputs 68% savings, 2 secrets blocked. |
| Show HN draft | `docs/launch/show-hn-draft.md` | Placeholders filled: 68%, 2 secrets. Ready to post. |

---

## ❌ Not Yet Built — Priority Order

### 1. README strategic reframe *(30 min, changes the pitch)*
- **What:** Change the one-line description from "a proxy that reduces API costs" to "the local security and efficiency layer for MCP-native agentic AI." Update the intro paragraph to lead with MCP security (injection scanner) before cost savings.
- **Why:** MCP security is the timely, differentiated angle. Cost savings is table stakes.

### 5. Eval-suite results baseline *(pre-launch hygiene)*
- **What:** Run `make eval-suite` against both binaries. Commit `benchmarks/eval-suite/RESULTS.md` with cache hit rate, redaction correctness, compression ratio.
- **Why:** Show HN audience will ask for numbers. Having them committed is better than saying "trust me."

---

## 🔮 Longer-Term (post-launch)

| Item | Notes |
|------|-------|
| Shared team cache | `KOTRO_CACHE_SERVER=http://shared-kotro:9001` — multiplies hit rate by team size |
| WASM plugin interface | Custom redaction/routing rules hot-loaded into `kotro-core`. Enables compliance customization without forking. |
| Observability suite | Pre-built Grafana dashboard JSON, per-request trace log, alerting rules (cache hit rate < 20%, budget at 80%) |
| Technical writeup | arXiv-style post on local semantic cache + AST-aware context pruning + circuit breaking. `docs/RUST-ARCHITECTURE.md` is the foundation. |
| Content flywheel | Problem-aware posts: "why is my Cursor bill so high", honest Kotro vs LiteLLM vs Portkey comparison |
| Design partners | 2–3 teams with quotable results ("cut our bill by X%"). Stronger than raw stars. |

---

## 🚀 Pre-Launch Checklist

- [x] SHA-256 exact-match cache working end-to-end
- [x] PII redaction working (10 pattern types)
- [x] MCP injection scanner shipped
- [x] Demo script produces correct output (68% savings)
- [x] Show HN draft ready
- [x] GitHub storefront set
- [x] Reasoning model budget controller shipped
- [x] Go declared frozen (tag `v0.1.0-go`, README updated, CI = compile-only)
- [x] `make eval-suite` results committed (RESULTS.md enriched with summary, methodology, Rust test coverage table)
- [x] `brew install kotro-labs/tap/kotro-proxy` — formula name corrected; bump tap to v0.5.2 before Show HN
- [x] `curl` installer verified — `curl | bash` installs to `~/.local/bin/kotro-proxy`, no sudo needed

---

## Session Notes

- **Cache hit header:** `x-kotro-cache: HIT` (lowercase, not `X-Kortolabs-Cache`)
- **Demo uses ports :8080 (proxy) and :9000 (mock).** Kill stale listeners before running: `lsof -ti:8080 | xargs kill -9 2>/dev/null || true`
- **Rust binary:** `bin/kotro-proxy` — built via `make proxy` or `make build`
- **Vector cache disabled in demo** (`KOTRO_ENABLE_VECTOR_CACHE=false`) to prevent over-matching on similar Rust prompts
- **spawn_blocking race:** cache write is fire-and-forget; script sleeps 300ms after each MISS to ensure DB write completes before next identical request
