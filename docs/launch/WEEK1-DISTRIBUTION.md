# Week 1 — Distribution Plan (source of truth)

**Strategy:** Stop backend surface. Ship one viral moment.  
**Sequence owner:** follow this checklist; do not jump to WASM / Cloud / comparison benches.

Last updated: 2026-07-14

---

## Status

| Day | Goal | Status |
|-----|------|--------|
| **1–2** | Security dashboard tiles + honest naming + USD rate | **DONE** (`6f961a9`, `034fcb3`) |
| **3–4** | Honest MCP injection exploit demo (script + 60–90s recording) | **DONE** — assets in `docs/launch/assets/` |
| **5** | Show HN — security first, savings second | **TODO** |
| Later | r/cursor, r/LocalLLaMA, bare vs Kotro table | Week 2+ |
| Skip this month | WASM seeding, Kotro Cloud, LiteLLM comparison benches | Deferred |

---

## Day 1–2 — Dashboard security tiles (DONE)

### Shipped
- Counters: `kotro_injections_detected_total`, `kotro_injections_blocked_total`, `kotro_agent_loops_stopped_total`, `kotro_budget_hits_total`
- Wired in OpenAI + Anthropic handlers
- Dashboard cards: **Injections Detected**, **Agent Loops Stopped**, **Budget Hits**
- Subtitle `(N blocked)` only when hard blocks > 0
- Default USD rate `$0.000015`; override via `KOTRO_DASHBOARD_USD_PER_TOKEN`
- Unit tests in `metrics::tests`

### Verify before Day 3
```bash
# Rebuild/run Rust proxy, then force each tile if needed:
# - injection: send tool/user message containing "ignore previous instructions"
# - loop: 3+ identical tool calls (or circuit breaker path)
# - budget: set KOTRO_SESSION_TOKEN_BUDGET low + hit cache miss
open http://127.0.0.1:9090/dashboard
```

---

## Day 3–4 — Honest exploit demo (DONE)

### Shipped
- Fixture: `docs/launch/fixtures/malicious-readme.md` (dummy secrets only)
- Repro: `make demo-injection` / `scripts/demo-injection.sh`
  - Phase A warn: HTTP 200 + `x-kotro-injection-warning` + Detected ≥ 1
  - Phase B block: **HTTP 400** + Blocked ≥ 1 (+ Anthropic `/v1/messages` smoke)
- Recording guide: `docs/launch/exploit-demo.md`
- Note: demo runs the cargo release artifact (`bin/rust-target/release/kotro-proxy`); Makefile `proxy` re-signs `bin/kotro-proxy` after copy (macOS dyld hang fix)

### Verify
```bash
make demo-injection
# optional faster re-run after a successful build:
# KOTRO_SKIP_REBUILD=1 bash scripts/demo-injection.sh
open http://127.0.0.1:9090/dashboard   # during the ~8s hold at end
```

### Assets
- [x] Screen recording (~78s narrated + silent) — `docs/launch/assets/exploit-demo-recording.mp4` / `*-silent.mp4`
- [x] Dashboard screenshot — `docs/launch/assets/dashboard-injection-demo.png` (red `BLOCKED` pills, non-zero $)
- [x] Snapshot JSON coherent — `docs/launch/assets/dashboard-snapshot.json`
- [x] Plain-text terminal — `docs/launch/assets/demo-injection-terminal.txt`

Rebuild narrated/silent cuts: `bash scripts/build-demo-recording.sh`

### Post-review fixes (publish gate)
- Blocked paths call `record_request(..., "blocked", …)` before early return (OpenAI + Anthropic)
- Dashboard `BLOCKED` pills use dedicated red severity styling (not grey bypass)
- Phase 0 warm-up (streamed) after Phase B restart so hero $ + traffic table are non-empty
- Narrated demo cut (macOS `say`) with silent fallback for muted viewing

### Framing (do not overclaim)
Kotro sits on the **HTTP path** between the agent and the LLM provider.  
When Claude Code / Cursor includes a poisoned tool or file result in the next `/v1/messages` (or chat completions) body, Kotro’s scanner sees it.

**Honest one-liner:**  
“Poisoned tool/file content rides into the next API call → Kotro detects → warns (default) or blocks (`KOTRO_INJECTION_BLOCK=true`).”

Do **not** claim MCP stdio intercept unless that hop is actually implemented.

### Deliverables
- [x] Repro script + docs under `docs/launch/` (`scripts/demo-injection.sh`, `exploit-demo.md`)
- [x] Screen recording — `docs/launch/assets/exploit-demo-recording.mp4` (~75s)
- [x] Dashboard screenshot — `docs/launch/assets/dashboard-injection-demo.png`

---

## Day 5 — Show HN

### Title (security first)
> Show HN: Kotro – local firewall for Claude Code and Cursor that blocks MCP prompt injection

### Body order
1. Exploit story (honest path)
2. Dashboard screenshot (Detected / Loops / Budget Hits + $)
3. Savings as habit / secondary (68% demo ok if reproducible)
4. Install: brew / curl / Marketplace
5. Real question (e.g. MiniLM ~26ms overhead worth it?)

### Pre-post checklist
- [ ] Tile labels match warn vs block behavior
- [ ] Status codes in copy: injection **400**, budget **429**
- [ ] Fresh-machine install works
- [ ] Post Tue/Wed **8–10am US Eastern**
- [ ] Repo URL as submission link

Draft live file: [`show-hn-draft.md`](./show-hn-draft.md) — update to match this plan before posting.

---

## Explicitly out of scope this month

- WASM plugin ecosystem seeding
- Kotro Cloud / managed tier
- Bare vs LiteLLM vs Kotro bake-off (Week 2 earliest, after HN)

---

## How agents should use this

When the user asks “what’s next” or continues launch work:
1. Read this file.
2. Drive the first incomplete **TODO** row only.
3. Do not start deferred items unless the user explicitly overrides.
