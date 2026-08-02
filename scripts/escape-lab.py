#!/usr/bin/env python3
"""Kotro Escape Lab runner.

Executes the checked-in scenario corpus against a running Kotro proxy and reports,
per scenario: outcome (prevent / transform / detect / observe / none), latency, and whether
tamper-evident evidence exists on the flight recorder tape.

The corpus declares the outcome each scenario is expected to produce *today*,
including honest no-coverage cases. The runner fails when observed behaviour
diverges from the declaration in either direction: a regression (coverage lost)
and an undeclared improvement (coverage gained) both require a deliberate
corpus update. That makes the matrix a regression gate rather than a brochure.

Usage:
    # Offline: schema + invariant checks only. No proxy required.
    python3 scripts/escape-lab.py --validate

    # Live run against a proxy.
    python3 scripts/escape-lab.py --target http://127.0.0.1:8080 \
        --control-token "$KOTRO_CONTROL_TOKEN" \
        --out results.json --markdown docs/security/ESCAPE-LAB-MATRIX.md
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "testdata" / "escape-lab" / "scenarios.json"
SCHEMA = ROOT / "testdata" / "escape-lab" / "scenario.schema.json"

OUTCOMES = ("prevent", "transform", "detect", "observe", "none")

# Flight kinds that represent a security verdict. Generic traffic kinds
# (`request`, `cache_hit`, `cache_miss`) are deliberately excluded: logging that
# an action happened is not the same as covering it, and counting it as coverage
# would make the matrix flatter to Kotro than the truth.
SECURITY_KINDS = frozenset(
    {
        "injection",
        "chain_alert",
        "tool_drift",
        "tool_denied",
        "posture_finding",
        "circuit_open",
        "tool_loop",
        "tool_storm",
        "budget",
        "kill_switch",
        "rate_limit",
    }
)
CATEGORIES = (
    "injection",
    "secret-exfiltration",
    "resource-abuse",
    "tool-integrity",
    "operator-control",
    "egress",
    "monitoring-integrity",
    "persistence",
)

GREEN, RED, YELLOW, DIM, BOLD, RESET = (
    "\033[0;32m",
    "\033[0;31m",
    "\033[1;33m",
    "\033[2m",
    "\033[1m",
    "\033[0m",
)


# ── corpus loading and offline validation ────────────────────────────────────


def load_corpus() -> list[dict[str, Any]]:
    with CORPUS.open(encoding="utf-8") as fh:
        data = json.load(fh)
    if not isinstance(data, list):
        raise SystemExit(f"{CORPUS}: expected a top-level array")
    return data


def validate_corpus(scenarios: list[dict[str, Any]]) -> list[str]:
    """Structural checks that do not require a live proxy.

    Deliberately hand-rolled rather than pulling a jsonschema dependency: the
    corpus is small, CI should not need a Python package install, and the
    invariants below (unique stable IDs, gap_reason on every no-coverage entry)
    are project rules that a generic validator would not enforce anyway.
    """
    errors: list[str] = []
    seen: set[str] = set()

    for idx, sc in enumerate(scenarios):
        where = sc.get("id", f"index {idx}")

        for field in ("id", "name", "category", "severity", "description", "attack", "expect"):
            if field not in sc:
                errors.append(f"{where}: missing required field '{field}'")

        sid = sc.get("id", "")
        if not (sid.startswith("EL-") and len(sid) == 5 and sid[3:].isdigit()):
            errors.append(f"{where}: id must match EL-NN")
        if sid in seen:
            errors.append(f"{where}: duplicate id (IDs are stable and never reused)")
        seen.add(sid)

        if sc.get("category") not in CATEGORIES:
            errors.append(f"{where}: unknown category {sc.get('category')!r}")
        if sc.get("severity") not in ("low", "medium", "high", "critical"):
            errors.append(f"{where}: unknown severity {sc.get('severity')!r}")

        expect = sc.get("expect", {})
        outcome = expect.get("outcome")
        if outcome not in OUTCOMES:
            errors.append(f"{where}: expect.outcome must be one of {OUTCOMES}")
        if outcome == "none" and not expect.get("gap_reason"):
            errors.append(
                f"{where}: outcome 'none' requires gap_reason naming the compensating "
                f"control or the phase that addresses it"
            )

        if sc.get("harness", "http") not in ("http", "cli"):
            errors.append(f"{where}: harness must be 'http' or 'cli'")

        steps = sc.get("attack", {}).get("steps", [])
        if not steps:
            errors.append(f"{where}: attack.steps must contain at least one step")

        for phase in ("setup", "attack", "teardown"):
            phase_steps = steps if phase == "attack" else (sc.get(phase) or [])
            for sidx, step in enumerate(phase_steps):
                if "method" not in step or "path" not in step:
                    errors.append(f"{where} {phase}[{sidx}]: method and path are required")
                if step.get("repeat", 1) < 1:
                    errors.append(f"{where} {phase}[{sidx}]: repeat must be >= 1")

        # Setup mutates proxy state that outlives the scenario. Without teardown
        # the corpus becomes order-dependent, which would make failures
        # reproduce only in the order CI happened to run them.
        if sc.get("setup") and not sc.get("teardown"):
            errors.append(
                f"{where}: setup without teardown — persistent state would leak into "
                f"later scenarios and make results order-dependent"
            )

    return errors


# ── HTTP ─────────────────────────────────────────────────────────────────────


def http_call(
    base: str, step: dict[str, Any], timeout: float
) -> tuple[int, dict[str, str], str]:
    url = base.rstrip("/") + step["path"]
    body = step.get("body")
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=step["method"])
    for key, val in (step.get("headers") or {}).items():
        req.add_header(key, val)

    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            headers = {k.lower(): v for k, v in resp.headers.items()}
            return resp.status, headers, resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as exc:
        headers = {k.lower(): v for k, v in exc.headers.items()}
        return exc.code, headers, exc.read().decode("utf-8", "replace")
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        return 0, {}, f"transport error: {exc}"


def fetch_observed_mode(control_base: str, token: str | None, timeout: float) -> str | None:
    """Read enforcement_mode from /api/runtime-posture (preferred) or kill-switch."""
    headers = {}
    if token:
        headers["x-kotro-control-token"] = token
    status, _, body = http_call(
        control_base, {"method": "GET", "path": "/api/runtime-posture", "headers": headers}, timeout
    )
    if status == 200:
        try:
            parsed = json.loads(body)
            mode = parsed.get("enforcement_mode") or (parsed.get("kill_switch") or {}).get("mode")
            if isinstance(mode, str) and mode.strip():
                return mode.strip().lower()
        except json.JSONDecodeError:
            pass
    status, _, body = http_call(
        control_base, {"method": "GET", "path": "/api/kill-switch", "headers": headers}, timeout
    )
    if status == 200:
        try:
            parsed = json.loads(body)
            mode = parsed.get("enforcement_mode") or parsed.get("mode")
            if isinstance(mode, str) and mode.strip():
                return mode.strip().lower()
        except json.JSONDecodeError:
            pass
    return None


def security_events_since(tape: list[dict[str, Any]], since_seq: int) -> list[dict[str, Any]]:
    return [
        e
        for e in tape
        if int(e.get("seq", 0)) > since_seq and e.get("kind") in SECURITY_KINDS
    ]


def fetch_tape(control_base: str, token: str | None, timeout: float) -> list[dict[str, Any]]:
    """Read the flight recorder tape from the control/metrics listener (:9090)."""
    step = {"method": "GET", "path": "/api/flight-recorder", "headers": {}}
    if token:
        step["headers"]["x-kotro-control-token"] = token
    status, _, body = http_call(control_base, step, timeout)
    if status != 200:
        return []
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError:
        return []
    if isinstance(parsed, list):
        return parsed
    for key in ("events", "recent", "items"):
        if isinstance(parsed.get(key), list):
            return parsed[key]
    return []


# ── outcome classification ───────────────────────────────────────────────────


def classify(
    scenario: dict[str, Any],
    status: int,
    headers: dict[str, str],
    body: str,
    tape_event: dict[str, Any] | None,
) -> str:
    """Derive the observed outcome from response and evidence.

    Ordering mirrors kotro-types::Verdict: blocked requests are 'prevent';
    redaction (harm removed while the request proceeds) is 'transform'; a
    warning without mutation is 'detect'.
    """
    expect = scenario.get("expect", {})

    blocked = status in (400, 403, 429) or "x-kotro-kill-switch" in headers
    if blocked and tape_event and tape_event.get("enforced"):
        return "prevent"
    if blocked:
        return "prevent"

    # Transform: harmful content removed. Prefer tape evidence; the response
    # header is corroborating but not sufficient alone for a published matrix.
    redactions = int((tape_event or {}).get("redaction_count") or 0)
    min_redactions = int(expect.get("min_redaction_count") or 0)
    if redactions > 0 and (min_redactions == 0 or redactions >= min_redactions):
        return "transform"
    if min_redactions > 0 and redactions >= min_redactions:
        return "transform"

    flagged = any(
        h in headers
        for h in (
            "x-kotro-injection-warning",
            "x-kotro-circuit-open",
        )
    )
    if flagged:
        return "detect"

    if tape_event is not None:
        kind = tape_event.get("kind", "")
        if tape_event.get("enforced"):
            return "prevent"
        if kind in SECURITY_KINDS:
            return "detect"
        # A `request` / `cache_*` event is traffic logging, not security coverage.
        # Falling through here keeps generic telemetry from inflating the matrix.

    # No verdict on the tape. A scenario can still surface the condition through
    # a reporting endpoint — posture is the case that matters today. That counts
    # as 'observe' only when the declared marker is actually present in the body.
    markers = expect.get("body_contains") or []
    if markers and status == 200 and all(m.lower() in body.lower() for m in markers):
        return "observe"

    # A declared flight_kind that never appeared is a real gap.
    return "none"


def match_tape_event(
    tape: list[dict[str, Any]], scenario: dict[str, Any], since_seq: int
) -> dict[str, Any] | None:
    expect = scenario.get("expect", {})
    want = expect.get("flight_kind")
    min_redactions = int(expect.get("min_redaction_count") or 0)
    candidates = [e for e in tape if int(e.get("seq", 0)) > since_seq]
    if min_redactions > 0:
        for event in reversed(candidates):
            if int(event.get("redaction_count") or 0) >= min_redactions:
                if want and event.get("kind") != want:
                    continue
                return event
        return None
    if want:
        for event in candidates:
            if event.get("kind") == want:
                return event
        return None
    return candidates[-1] if candidates else None


# ── run ──────────────────────────────────────────────────────────────────────


def step_base(path: str, proxy_base: str, control_base: str) -> str:
    """Control/telemetry APIs live on the metrics listener, not the LLM proxy."""
    if path.startswith("/api/") or path.startswith("/dashboard") or path == "/metrics":
        return control_base
    return proxy_base


def run_steps(
    steps: list[dict[str, Any]],
    proxy_base: str,
    control_base: str,
    token: str | None,
    timeout: float,
) -> tuple[int, dict[str, str], str]:
    """Execute steps sequentially, returning the final response.

    `/api/*` (and dashboard/metrics) go to `control_base` (:9090). LLM routes
    go to `proxy_base` (:8080). The control token is attached when provided.
    """
    status, headers, body = 0, {}, ""
    for step in steps:
        if token:
            step = dict(step)
            step["headers"] = {"x-kotro-control-token": token, **(step.get("headers") or {})}
        base = step_base(step["path"], proxy_base, control_base)
        for _ in range(step.get("repeat", 1)):
            status, headers, body = http_call(base, step, timeout)
    return status, headers, body


def run_scenario(
    scenario: dict[str, Any],
    proxy_base: str,
    control_base: str,
    token: str | None,
    timeout: float,
) -> dict[str, Any]:
    setup_failed = ""
    if scenario.get("setup"):
        st, _, sb = run_steps(scenario["setup"], proxy_base, control_base, token, timeout)
        if st not in (200, 202, 204):
            setup_failed = f"setup returned {st}: {sb[:100]}"

    before = fetch_tape(control_base, token, timeout)
    since_seq = max((int(e.get("seq", 0)) for e in before), default=0)

    started = time.perf_counter()
    try:
        status, headers, body = run_steps(
            scenario["attack"]["steps"], proxy_base, control_base, token, timeout
        )
    finally:
        latency_ms = round((time.perf_counter() - started) * 1000, 1)
        # Teardown always runs. Setup mutates persistent proxy state (the kill
        # switch survives the request that engaged it), so skipping teardown on
        # failure would silently poison every scenario that follows.
        if scenario.get("teardown"):
            run_steps(scenario["teardown"], proxy_base, control_base, token, timeout)

    after = fetch_tape(control_base, token, timeout)
    tape_reachable = bool(after) or bool(before)
    event = match_tape_event(after, scenario, since_seq)
    header_mode = (headers.get("x-kotro-mode") or "").strip().lower() or None

    observed = classify(scenario, status, headers, body, event)
    expected = scenario["expect"]["outcome"]

    detail: list[str] = []
    if setup_failed:
        detail.append(setup_failed)
    for marker in scenario["expect"].get("body_contains", []):
        if marker.lower() not in body.lower():
            detail.append(f"body missing {marker!r}")
    for name in scenario["expect"].get("headers_present", []):
        if name.lower() not in headers:
            detail.append(f"missing header {name}")
    for name in scenario["expect"].get("headers_absent", []):
        if name.lower() in headers:
            detail.append(f"unexpected header {name}")
    want_mode = scenario["expect"].get("kotro_mode")
    if want_mode:
        got_mode = header_mode or ""
        if got_mode != str(want_mode).strip().lower():
            detail.append(f"x-kotro-mode {got_mode!r} != {want_mode!r}")
    allowed = scenario["expect"].get("http_status")
    if allowed and status not in allowed:
        detail.append(f"status {status} not in {allowed}")
    min_redactions = int(scenario["expect"].get("min_redaction_count") or 0)
    if min_redactions:
        got = int((event or {}).get("redaction_count") or 0)
        if got < min_redactions:
            detail.append(f"redaction_count {got} < {min_redactions}")
    if "flight_enforced" in scenario["expect"]:
        want_enf = bool(scenario["expect"]["flight_enforced"])
        if event is None:
            detail.append("missing flight event for flight_enforced check")
        elif bool(event.get("enforced")) != want_enf:
            detail.append(
                f"flight enforced={event.get('enforced')!r} != {want_enf}"
            )
    if scenario["expect"].get("no_security_events"):
        leaked = security_events_since(after, since_seq)
        if leaked:
            kinds = ",".join(sorted({str(e.get("kind")) for e in leaked}))
            detail.append(f"unexpected security events: {kinds}")
    if status == 0:
        detail.append(body[:120])

    matched = observed == expected and not detail

    return {
        "id": scenario["id"],
        "name": scenario["name"],
        "category": scenario["category"],
        "severity": scenario["severity"],
        "env_group": scenario.get("env_group", "default"),
        "expected": expected,
        "observed": observed,
        "pass": matched,
        "http_status": status,
        "latency_ms": latency_ms,
        "kotro_mode": header_mode,
        "mode_dial": bool(scenario["expect"].get("mode_dial")),
        "evidence": {
            "tape_reachable": tape_reachable,
            "event_kind": (event or {}).get("kind"),
            "enforced": (event or {}).get("enforced"),
            "redaction_count": (event or {}).get("redaction_count"),
            "chain_hash": (event or {}).get("hash", "")[:16] or None,
            "x_kotro_mode": header_mode,
        },
        "notes": "; ".join(detail),
        "gap_reason": scenario["expect"].get("gap_reason"),
    }


# ── reporting ────────────────────────────────────────────────────────────────


def render_markdown(
    results: list[dict[str, Any]],
    target: str,
    *,
    env_group: str | None = None,
    observed_mode: str | None = None,
) -> str:
    measured = [r for r in results if not r.get("skipped")]
    total = len(measured)
    passed = sum(1 for r in measured if r["pass"])
    covered = sum(1 for r in measured if r["observed"] in ("prevent", "transform", "detect"))
    modes = sorted({r.get("kotro_mode") for r in measured if r.get("kotro_mode")})
    mode_note = observed_mode or (modes[0] if len(modes) == 1 else None)
    if not mode_note and modes:
        mode_note = ",".join(modes)
    groups = sorted({r.get("env_group") for r in results if r.get("env_group")})
    group_note = env_group or (",".join(g for g in groups if g) or None)

    lines = [
        "<!-- Generated by Escape Lab live matrix run. Do not edit by hand. -->",
        f"<!-- groups: {group_note or 'unknown'} -->",
        f"<!-- kotro_mode: {mode_note or 'unknown'} -->",
        "# Kotro Escape Lab — coverage matrix",
        "",
        "Generated by `scripts/escape-lab.py`. Do not edit by hand.",
        "",
        f"Scenarios: **{total}** · matching declared behaviour: **{passed}/{total}** · "
        f"prevented, transformed, or detected: **{covered}/{total}**",
        "",
    ]
    if mode_note or group_note:
        lines.append(
            f"**Measured under** `KOTRO_MODE` / `x-kotro-mode`: **`{mode_note or 'unknown'}`**"
            + (f" · env group: `{group_note}`" if group_note else "")
            + "."
        )
        lines.append("")
        lines.append(
            "Outcomes that say `prevent` for injection assume `KOTRO_MODE=enforce`. "
            "At `audit` the same payload is `detect`; at `disabled` it is `none` "
            "(see EL-12 / EL-13)."
        )
        lines.append("")
    lines += [
        "`prevent` = blocked before effect. `transform` = allowed, harmful content removed. "
        "`detect` = allowed intact, flagged with evidence. "
        "`observe` = recorded without a verdict. `none` = no coverage today.",
        "",
        "| ID | Scenario | Category | Severity | Mode | Outcome | Latency | Evidence |",
        "|----|----------|----------|----------|------|---------|---------|----------|",
    ]
    for r in results:
        ev = r["evidence"]
        evidence = f"`{ev['event_kind']}`" if ev.get("event_kind") else "—"
        mode = r.get("kotro_mode") or "—"
        if r.get("skipped"):
            lines.append(
                f"| {r['id']} | {r['name']} | {r['category']} | {r['severity']} | "
                f"— | _not measured here_ | — | — |"
            )
            continue
        lines.append(
            f"| {r['id']} | {r['name']} | {r['category']} | {r['severity']} | "
            f"`{mode}` | **{r['observed']}** | {r['latency_ms']} ms | {evidence} |"
        )

    skipped = [r for r in results if r.get("skipped")]
    if skipped:
        lines += ["", "### Not measured by this harness", ""]
        for r in skipped:
            lines.append(f"- **{r['id']} — {r['name']}**: {r['notes']}")
        lines.append("")

    dial_rows = [r for r in results if r.get("mode_dial") and not r.get("skipped")]
    gaps = [
        r
        for r in results
        if r["observed"] == "none" and not r.get("mode_dial") and not r.get("skipped")
    ]
    if dial_rows:
        lines += ["", "## Mode dial assertions", ""]
        for r in dial_rows:
            lines.append(f"**{r['id']} — {r['name']}**  ")
            lines.append(f"{r.get('gap_reason') or 'Asserts KOTRO_MODE dial behaviour.'}")
            lines.append("")
    if gaps:
        lines += ["", "## Known gaps", ""]
        for r in gaps:
            lines.append(f"**{r['id']} — {r['name']}**  ")
            lines.append(f"{r.get('gap_reason') or 'No stated reason.'}")
            lines.append("")

    lines += [
        "## Method",
        "",
        f"Each scenario is replayed against a live proxy at `{target}` from the corpus in "
        "`testdata/escape-lab/scenarios.json`. Outcome is derived from the response status, "
        "Kotro guardrail headers (including `x-kotro-mode`), and the matching flight recorder "
        "event. The matrix header records the observed enforcement mode so prevent/detect "
        "rows stay reproducible after C7. Latency covers the full scenario including repeated "
        "steps, so multi-step scenarios are not comparable to single-request ones.",
        "",
        "Scenarios declaring `none` are tracked gaps with a stated compensating control or "
        "owning phase. They are expected to fail and the run is green when they do — the "
        "corpus is a regression gate, not a scorecard.",
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description="Kotro Escape Lab runner")
    ap.add_argument("--validate", action="store_true", help="offline corpus checks only")
    ap.add_argument("--target", default="http://127.0.0.1:8080",
                    help="LLM proxy base (chat/completions, healthz)")
    ap.add_argument(
        "--control-target",
        default="http://127.0.0.1:9090",
        help="control/metrics base for /api/*, flight tape, posture, kill-switch",
    )
    ap.add_argument("--control-token", default=os.environ.get("KOTRO_CONTROL_TOKEN"))
    ap.add_argument("--timeout", type=float, default=15.0)
    ap.add_argument("--out", help="write results JSON here")
    ap.add_argument("--markdown", help="write the coverage matrix here")
    ap.add_argument(
        "--env-group",
        default="default",
        help="run only scenarios in this env group (default: 'default'). Groups exist "
        "because configs like injection warn and block are mutually exclusive.",
    )
    ap.add_argument(
        "--list-groups", action="store_true", help="print env groups and exit"
    )
    ap.add_argument(
        "--allow-divergence",
        action="store_true",
        help="report divergence without failing the run",
    )
    args = ap.parse_args()

    scenarios = load_corpus()

    errors = validate_corpus(scenarios)
    if errors:
        print(f"{RED}Corpus validation failed:{RESET}", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1
    print(f"{GREEN}✓{RESET} corpus valid — {len(scenarios)} scenarios, schema at {SCHEMA.name}")

    if args.list_groups:
        seen: dict[str, list[str]] = {}
        for sc in scenarios:
            seen.setdefault(sc.get("env_group", "default"), []).append(sc["id"])
        for group, ids in sorted(seen.items()):
            env = next(
                (s.get("env") for s in scenarios if s.get("env_group", "default") == group and s.get("env")),
                {},
            )
            env_str = " ".join(f"{k}={v}" for k, v in (env or {}).items()) or "(proxy defaults)"
            print(f"  {group:<18} {','.join(ids):<36} {DIM}{env_str}{RESET}")
        return 0

    if args.validate:
        return 0

    selected = [s for s in scenarios if s.get("env_group", "default") == args.env_group]
    if not selected:
        groups = sorted({s.get("env_group", "default") for s in scenarios})
        print(f"{RED}✗{RESET} no scenarios in env group {args.env_group!r}; have {groups}", file=sys.stderr)
        return 1

    probe, _, _ = http_call(args.target, {"method": "GET", "path": "/healthz"}, args.timeout)
    if probe != 200:
        print(f"{RED}✗{RESET} no proxy at {args.target} (/healthz returned {probe})", file=sys.stderr)
        return 2

    if not args.control_token:
        print(
            f"{YELLOW}!{RESET} no control token — evidence checks will be reported as unverified"
        )

    ctrl_probe, _, _ = http_call(
        args.control_target,
        {"method": "GET", "path": "/api/flight-recorder",
         "headers": {"x-kotro-control-token": args.control_token or ""}},
        args.timeout,
    )
    if ctrl_probe not in (200, 401, 403):
        print(
            f"{YELLOW}!{RESET} control plane at {args.control_target} looks unreachable "
            f"(/api/flight-recorder returned {ctrl_probe})",
            file=sys.stderr,
        )

    observed_mode = fetch_observed_mode(args.control_target, args.control_token, args.timeout)
    mode_label = observed_mode or "unknown"
    print(
        f"{DIM}env group {args.env_group} — {len(selected)} scenario(s) · "
        f"KOTRO_MODE={mode_label} · "
        f"proxy {args.target} · control {args.control_target}{RESET}"
    )

    results = []
    for scenario in selected:
        # `cli` scenarios exercise mcp-wrap on MCP stdio, outside the HTTP path
        # this runner drives. Reporting them as gaps would understate coverage;
        # asserting a synthetic event we posted ourselves would overstate it.
        if scenario.get("harness", "http") == "cli":
            results.append(
                {
                    "id": scenario["id"],
                    "name": scenario["name"],
                    "category": scenario["category"],
                    "severity": scenario["severity"],
                    "env_group": scenario.get("env_group", "default"),
                    "expected": scenario["expect"]["outcome"],
                    "observed": "skipped",
                    "pass": True,
                    "skipped": True,
                    "http_status": None,
                    "latency_ms": 0.0,
                    "kotro_mode": None,
                    "mode_dial": bool(scenario["expect"].get("mode_dial")),
                    "evidence": {"tape_reachable": None, "event_kind": None,
                                 "enforced": None, "redaction_count": None,
                                 "chain_hash": None, "x_kotro_mode": None},
                    "notes": "cli harness — not measurable over HTTP",
                    "gap_reason": scenario["expect"].get("gap_reason"),
                }
            )
            print(f"  {YELLOW}–{RESET} {scenario['id']}  {DIM}skipped (cli harness){RESET}")
            continue

        res = run_scenario(
            scenario, args.target, args.control_target, args.control_token, args.timeout
        )
        res["skipped"] = False
        results.append(res)
        mark = f"{GREEN}✓{RESET}" if res["pass"] else f"{RED}✗{RESET}"
        line = (
            f"  {mark} {res['id']}  {res['observed']:<8} "
            f"{DIM}(declared {res['expected']}){RESET}  {res['latency_ms']} ms"
        )
        if res["notes"]:
            line += f"  {DIM}{res['notes']}{RESET}"
        print(line)

    diverged = [r for r in results if not r["pass"]]
    measured = [r for r in results if not r.get("skipped")]
    skipped = [r for r in results if r.get("skipped")]
    covered = sum(1 for r in measured if r["observed"] in ("prevent", "transform", "detect"))

    print()
    summary = (
        f"{BOLD}{len(measured) - len(diverged)}/{len(measured)}{RESET} match declared behaviour · "
        f"{covered}/{len(measured)} prevented, transformed, or detected"
    )
    if skipped:
        summary += f" · {len(skipped)} skipped"
    print(summary)

    if args.out:
        payload = {
            "target": args.target,
            "control_target": args.control_target,
            "env_group": args.env_group,
            "kotro_mode": observed_mode,
            "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "summary": {
                "total": len(results),
                "matching": len(results) - len(diverged),
                "covered": covered,
            },
            "results": results,
        }
        Path(args.out).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        print(f"{DIM}wrote {args.out}{RESET}")

    if args.markdown:
        Path(args.markdown).parent.mkdir(parents=True, exist_ok=True)
        Path(args.markdown).write_text(
            render_markdown(
                results,
                args.target,
                env_group=args.env_group,
                observed_mode=observed_mode,
            ),
            encoding="utf-8",
        )
        print(f"{DIM}wrote {args.markdown}{RESET}")

    if diverged:
        print()
        print(f"{RED}Divergence from the declared corpus:{RESET}", file=sys.stderr)
        for r in diverged:
            print(
                f"  {r['id']}: declared {r['expected']}, observed {r['observed']}"
                + (f" ({r['notes']})" if r["notes"] else ""),
                file=sys.stderr,
            )
        print(
            "\nEither the behaviour regressed, or coverage improved and "
            "testdata/escape-lab/scenarios.json needs a deliberate update.",
            file=sys.stderr,
        )
        return 0 if args.allow_divergence else 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
