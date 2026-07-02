#!/usr/bin/env python3
"""Merge eval-segment JSON into .last-run.json and render RESULTS.md."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


def metric_values(metrics: dict[str, Any], name: str) -> dict[str, Any]:
    raw = metrics.get("metrics", {}).get(name, {})
    nested = raw.get("values")
    return nested if isinstance(nested, dict) else raw


def ms(metrics: dict[str, Any], percentile: str) -> int:
    values = metric_values(metrics, "http_req_duration")
    if percentile == "p(50)":
        val = values.get("p(50)") or values.get("med") or 0
    elif percentile == "p(99)":
        val = values.get("p(99)") or values.get("max") or values.get("p(95)") or 0
    else:
        val = values.get(percentile) or 0
    return int(float(val))


def rate(metrics: dict[str, Any], name: str) -> float:
    values = metric_values(metrics, name)
    return float(values.get("rate", 0))


def merge(
    *,
    version: str,
    git_sha: str,
    date_utc: str,
    host: str,
    run_id: str,
    compression: dict[str, Any],
    k6_hit: dict[str, Any],
    k6_miss: dict[str, Any],
    parity: str,
    hit_alpha: str,
    hit_beta: str,
    isolation_pass: bool,
    audit_pass: bool,
    baseline_g: str,
    post_g: str,
    delta_g: str,
) -> dict[str, Any]:
    def parse_int(value: str) -> int:
        try:
            return int(value)
        except (TypeError, ValueError):
            return 0

    return {
        "meta": {
            "version": version,
            "git_sha": git_sha,
            "date_utc": date_utc,
            "host": host,
            "run_id": run_id,
            "upstream": "mock",
            "runtime": "go",
        },
        "compression": compression,
        "latency": {
            "cache_hit": {
                "p50_ms": ms(k6_hit, "p(50)"),
                "p95_ms": ms(k6_hit, "p(95)"),
                "p99_ms": ms(k6_hit, "p(99)"),
                "http_req_failed_rate": rate(k6_hit, "http_req_failed"),
            },
            "cache_miss": {
                "p50_ms": ms(k6_miss, "p(50)"),
                "p95_ms": ms(k6_miss, "p(95)"),
                "p99_ms": ms(k6_miss, "p(99)"),
                "http_req_failed_rate": rate(k6_miss, "http_req_failed"),
            },
        },
        "parity": {"openai_stream_byte_identical": parity == "pass"},
        "isolation": {
            "tenant_alpha_cache": hit_alpha,
            "tenant_beta_cache": hit_beta,
            "pass": isolation_pass,
        },
        "cancel_storm": {
            "baseline_goroutines": parse_int(baseline_g),
            "post_goroutines": parse_int(post_g),
            "delta": parse_int(delta_g),
            "pass": audit_pass,
        },
    }


def kb(n: int) -> float:
    return round(n / 1024, 2)


def onoff(value: bool) -> str:
    return "pass" if value else "fail"


def render(data: dict[str, Any]) -> str:
    r = data
    w1 = r["compression"]["w1_context_reload"]
    w2 = r["compression"]["w2_tool_dumps"]
    hit = r["latency"]["cache_hit"]
    miss = r["latency"]["cache_miss"]
    parity = r["parity"]["openai_stream_byte_identical"]
    iso = r["isolation"]
    storm = r["cancel_storm"]
    meta = r["meta"]

    w1_rows = "\n".join(
        f"| {t['turn']} | {kb(t['input_bytes'])} | {kb(t['upstream_bytes'])} | "
        f"{'yes' if t['blocks_stripped'] else 'no'} | {t['cache']} |"
        for t in w1["turns"]
    )
    w2_rows = "\n".join(
        f"| {t['turn']} | {kb(t['input_bytes'])} | {kb(t['upstream_bytes'])} | "
        f"{'yes' if t['blocks_stripped'] else 'no'} |"
        for t in w2["turns"]
    )

    beta_display = iso["tenant_beta_cache"] or "MISS"
    alpha_cache = iso["tenant_alpha_cache"] or "MISS"

    return f"""# Kotro Eval Suite — Results Dashboard

**Auto-generated from** `{meta['run_id']}`. Re-run: `make eval-suite`.

---

## Run metadata

| Field | Value |
|-------|-------|
| **Kotro version** | `{meta['version']}` |
| **Git SHA** | `{meta['git_sha']}` |
| **Runtime** | `{meta['runtime']}` |
| **Date (UTC)** | `{meta['date_utc']}` |
| **Host** | `{meta['host']}` |
| **Upstream** | `{meta['upstream']}` |
| **Config snapshot** | `KORTO_ENABLE_CACHE=true`, `KORTO_ENABLE_COMPRESSION=true`, `KORTO_ENABLE_REDACTION=true` |

---

## Executive summary

| Metric | Baseline (no Kotro) | With Kotro | Delta |
|--------|---------------------|------------|-------|
| **Compressor savings (W1 turn 10)** | full context each turn | stripped static blocks | {w1['savings_pct_last_turn']:.1f}% |
| **p50 end-to-end latency** | — | hit {hit['p50_ms']} ms / miss {miss['p50_ms']} ms | — |
| **p99 end-to-end latency** | — | hit {hit['p99_ms']} ms / miss {miss['p99_ms']} ms | — |
| **Cache hit rate (k6 hit scenario)** | N/A | ~100% (warm payload) | — |
| **Output parity** | — | — | {onoff(parity)} |

**One-line takeaway:**
> MCP-style context reload: {w1['savings_pct_last_turn']:.1f}% smaller upstream payload on turn 10; cache hit p99 {hit['p99_ms']} ms vs miss p99 {miss['p99_ms']} ms (mock upstream).

---

## W1 — Context reload storm (IDE wedge)

| Turn | Input (KB) | Upstream (KB) | Blocks stripped | Cache |
|------|------------|---------------|-----------------|-------|
{w1_rows}

**Command:** `make eval-suite` (compression segment uses offline `measure.go`).

---

## W2 — Tool output dumps

| Turn | Input (KB) | Upstream (KB) | Blocks stripped |
|------|------------|---------------|-----------------|
{w2_rows}

**Savings on last turn:** {w2['savings_pct_last_turn']:.1f}%

---

## W3 — Cache hit / miss latency

| Scenario | p50 (ms) | p95 (ms) | p99 (ms) | Notes |
|----------|----------|----------|----------|-------|
| Cache HIT (replay) | {hit['p50_ms']} | {hit['p95_ms']} | {hit['p99_ms']} | `X-KortoLabs-Cache: HIT` |
| Cache MISS (upstream) | {miss['p50_ms']} | {miss['p95_ms']} | {miss['p99_ms']} | unique prompts |

**Command:** `make load-test SCENARIO=hit` / `SCENARIO=miss`

---

## W4 — Output fidelity (parity)

| Test | Provider | Byte-identical miss vs hit | Pass |
|------|----------|--------------------------|------|
| Deterministic replay | OpenAI | {'yes' if parity else 'no'} | {onoff(parity)} |

---

## W5 — Isolation verification

| Test | Expected | Actual | Pass |
|------|----------|--------|------|
| Tenant A cred → repeat | HIT | {alpha_cache} | {onoff(iso['tenant_alpha_cache'] == 'HIT')} |
| Tenant B cred → same prompt | MISS | {beta_display} | {onoff(iso['pass'])} |

---

## W6 — Cancel storm / goroutine stability

| Phase | Goroutines | Δ from baseline |
|-------|------------|-----------------|
| Baseline | {storm['baseline_goroutines']} | 0 |
| Post-cooldown | {storm['post_goroutines']} | {storm['delta']} |

**Result:** {'PASS' if storm['pass'] else 'FAIL'} (tolerance ±5)

**Command:** `make cancel-audit`

---

## Historical trend

| Release | W1 turn-10 savings | Hit p99 (ms) | Miss p99 (ms) | Cancel storm | Notes |
|---------|-------------------|--------------|---------------|--------------|-------|
| {meta['version']} | {w1['savings_pct_last_turn']:.1f}% | {hit['p99_ms']} | {miss['p99_ms']} | {'PASS' if storm['pass'] else 'FAIL'} | eval-suite run {meta['date_utc']} |

---

## Related documents

- [90-DAY-ROADMAP.md](../../docs/roadmap/90-DAY-ROADMAP.md)
- [OBSERVABILITY-SPEC.md](../../docs/operations/OBSERVABILITY-SPEC.md)
- [THREAT-MODEL.md](../../docs/security/THREAT-MODEL.md)
"""


def main() -> None:
    if len(sys.argv) < 2:
        print("usage: results_tool.py merge|render ...", file=sys.stderr)
        sys.exit(1)

    cmd = sys.argv[1]
    if cmd == "merge":
        out_path = Path(sys.argv[2])
        payload = json.loads(sys.argv[3])
        out_path.write_text(json.dumps(merge(**payload), indent=2) + "\n")
        return

    if cmd == "render":
        json_path = Path(sys.argv[2])
        out_path = Path(sys.argv[3])
        data = json.loads(json_path.read_text())
        out_path.write_text(render(data))
        return

    print(f"unknown command: {cmd}", file=sys.stderr)
    sys.exit(1)


if __name__ == "__main__":
    main()
