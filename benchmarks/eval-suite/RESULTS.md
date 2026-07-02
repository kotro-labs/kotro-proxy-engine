# Kotro Eval Suite — Results Dashboard

**Auto-generated from** `20260702T045418Z`. Re-run: `make eval-suite`.

---

## Run metadata

| Field | Value |
|-------|-------|
| **Kotro version** | `v0.1.2-1-g6e8842e` |
| **Git SHA** | `6e8842e` |
| **Runtime** | `go` |
| **Date (UTC)** | `2026-07-02` |
| **Host** | `Darwin 23.4.0 arm64` |
| **Upstream** | `mock` |
| **Config snapshot** | `KORTO_ENABLE_CACHE=true`, `KORTO_ENABLE_COMPRESSION=true`, `KORTO_ENABLE_REDACTION=true` |

---

## Executive summary

| Metric | Baseline (no Kotro) | With Kotro | Delta |
|--------|---------------------|------------|-------|
| **Compressor savings (W1 turn 10)** | full context each turn | stripped static blocks | 99.4% |
| **p50 end-to-end latency** | — | hit 20 ms / miss 196 ms | — |
| **p99 end-to-end latency** | — | hit 68 ms / miss 426 ms | — |
| **Cache hit rate (k6 hit scenario)** | N/A | ~100% (warm payload) | — |
| **Output parity** | — | — | pass |

**One-line takeaway:**
> MCP-style context reload: 99.4% smaller upstream payload on turn 10; cache hit p99 68 ms vs miss p99 426 ms (mock upstream).

---

## W1 — Context reload storm (IDE wedge)

| Turn | Input (KB) | Upstream (KB) | Blocks stripped | Cache |
|------|------------|---------------|-----------------|-------|
| 1 | 2.02 | 2.02 | no | MISS |
| 2 | 2.02 | 0.02 | yes | n/a |
| 3 | 2.02 | 0.01 | yes | n/a |
| 4 | 2.02 | 0.01 | yes | n/a |
| 5 | 2.02 | 0.01 | yes | n/a |
| 6 | 2.02 | 0.01 | yes | n/a |
| 7 | 2.02 | 0.02 | yes | n/a |
| 8 | 2.02 | 0.01 | yes | n/a |
| 9 | 2.02 | 0.01 | yes | n/a |
| 10 | 2.02 | 0.01 | yes | n/a |

**Command:** `make eval-suite` (compression segment uses offline `measure.go`).

---

## W2 — Tool output dumps

| Turn | Input (KB) | Upstream (KB) | Blocks stripped |
|------|------------|---------------|-----------------|
| 1 | 8.45 | 8.45 | no |
| 2 | 8.45 | 0.02 | yes |
| 3 | 8.44 | 0.01 | yes |

**Savings on last turn:** 99.9%

---

## W3 — Cache hit / miss latency

| Scenario | p50 (ms) | p95 (ms) | p99 (ms) | Notes |
|----------|----------|----------|----------|-------|
| Cache HIT (replay) | 20 | 21 | 68 | `X-KortoLabs-Cache: HIT` |
| Cache MISS (upstream) | 196 | 242 | 426 | unique prompts |

**Command:** `make load-test SCENARIO=hit` / `SCENARIO=miss`

---

## W4 — Output fidelity (parity)

| Test | Provider | Byte-identical miss vs hit | Pass |
|------|----------|--------------------------|------|
| Deterministic replay | OpenAI | yes | pass |

---

## W5 — Isolation verification

| Test | Expected | Actual | Pass |
|------|----------|--------|------|
| Tenant A cred → repeat | HIT | HIT | pass |
| Tenant B cred → same prompt | MISS | MISS | pass |

---

## W6 — Cancel storm / goroutine stability

| Phase | Goroutines | Δ from baseline |
|-------|------------|-----------------|
| Baseline | 8 | 0 |
| Post-cooldown | 8 | 0 |

**Result:** PASS (tolerance ±5)

**Command:** `make cancel-audit`

---

## Historical trend

| Release | W1 turn-10 savings | Hit p99 (ms) | Miss p99 (ms) | Cancel storm | Notes |
|---------|-------------------|--------------|---------------|--------------|-------|
| v0.1.2-1-g6e8842e | 99.4% | 68 | 426 | PASS | eval-suite run 2026-07-02 |

---

## Related documents

- [90-DAY-ROADMAP.md](../../docs/roadmap/90-DAY-ROADMAP.md)
- [OBSERVABILITY-SPEC.md](../../docs/operations/OBSERVABILITY-SPEC.md)
- [THREAT-MODEL.md](../../docs/security/THREAT-MODEL.md)
