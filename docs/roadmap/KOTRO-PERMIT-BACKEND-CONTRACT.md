# Kotro Permit — enforcement-backend contract (Sol 2026-08-07)

**Status:** Adopted. Docker remains the **reference** backend. Alternate backends are adapters that must pass this contract — not drop-in time savers.  
**Related:** [`KOTRO-PERMIT-SANDBOX.md`](./KOTRO-PERMIT-SANDBOX.md) · [`../launch/GATE-A-RECRUITING.md`](../launch/GATE-A-RECRUITING.md)

---

## Positioning reminder

> Nono, srt, or Docker enforce the boundary. Kotro makes the job authority portable, delegable, and provable.

Kotro does **not** need to win a sandbox bake-off. It needs a backend that meets the acceptance rows below so authority + receipts mean something.

---

## Security property that keeps Docker as reference

**Mount-namespace absence** (path never present → `ENOENT` / `FileNotFoundError`) is a **different** property from **path denial** (path present → `EACCES` / `PermissionError`).

Kotro’s tested alpha baseline (R0.1a) prefers **absence**. Any replacement backend must either:

- Preserve absence semantics for host secrets, **or**
- Explicitly document a **semantic mismatch** and get a product decision before replacing Docker as default.

Switching backends only saves time if the replacement **passes this contract**. Otherwise you trade implementation work for integration and semantic-mismatch work.

---

## Mandatory guarantees (all backends)

| # | Guarantee | Notes |
|---|-----------|--------|
| G1 | Host secrets not reachable by shell/Python inside the agent | Prefer ENOENT; document if only EACCES |
| G2 | Fail-closed if backend unavailable | **Never** warn-and-run unsandboxed |
| G3 | No provider / GitHub tokens in agent env or mounts | |
| G4 | Live host repo not RW-mounted (Option A) | |
| G5 | Deny public egress baseline (or stronger) | Dual-home honesty still applies |
| G6 | No docker.sock / host-control socket in agent | |
| G7 | Kotro can still mediate LLM / broker on host side | |

Acceptance rows for spikes: suite **#4/#5** (shell/Python secret), **#9** (unavailable → refuse), **#10** (token absent), plus topology rows already in R0.1a/R0.1b as applicable.

---

## Alpha default: Docker / OCI

| Item | Rule |
|------|------|
| Default backend | Docker Engine / Docker Desktop (native arch ≥ 4.x) |
| If unavailable | Refuse `run --permit` (exit fail-closed) |
| Role | **Reference** for acceptance — do not remove while evaluating adapters |

---

## Adapter: Anthropic sandbox-runtime (srt)

Claude Code’s sandbox defaults to **warn and run unsandboxed** when the sandbox is unavailable unless `sandbox.failIfUnavailable=true`.

That **directly contradicts** Kotro’s fail-closed invariant.

| Rule | Requirement |
|------|-------------|
| Precondition | Adapter **must refuse to start** unless `sandbox.failIfUnavailable=true` (or equivalent) is **confirmed** |
| Not optional | Do not inherit the permissive default “for convenience” |
| Unavailable | Same as Docker: no host fallback |

---

## Adapter candidate: Nono (time-boxed spike)

**Plan (Sol):** three-day spike against the **same acceptance rows**, Docker kept as reference. Do **not** replace Docker as default until the spike passes.

| Rule | Requirement |
|------|-------------|
| Scope | Spike only — measure ENOENT vs EACCES, #4/#5, fail-closed, egress posture |
| **Pin version hard** | Record exact Nono version / git SHA in the spike README and this contract |
| **API churn** | Pre-1.0 churn is a **scheduled cost**, not a surprise |
| **Dated recheck** | See calendar below — re-run acceptance on pin bump |

### Nono pin & recheck calendar

| Field | Value |
|-------|--------|
| Spike window | **3 calendar days** max once started |
| Initial pin | *TBD at spike start — fill version + commit SHA* |
| Recheck due | **2026-09-07** (or 30 days after spike start, whichever earlier) |
| On recheck | Re-run #4/#5/#9 against pinned version; if APIs broke, budget adapter update **before** claiming Nono-ready |
| Pass criterion | Same rows as Docker reference **or** explicit signed product decision accepting semantic mismatch |

Spike harness location (when created): `spikes/r-backend-nono/` (keep Docker results side-by-side).

---

## Factual hygiene

| Claim | Status |
|-------|--------|
| “Codex ships Landlock + seccomp **by default**” | **Unverified** — secondary sources only. **Do not** put in comparison docs until primary documentation is cited. |
| Pipelock Landlock/seccomp | Documented competitor capability — OK to cite for Pipelock specifically |
| Kotro Docker mount absence | Verified by R0.1a PASS |

---

## Decision rule

```
Docker reference green
    │
    ├─ Gate A wedge signal weak → pause adapters; no Nono/srt productization
    │
    └─ Gate A wedge signal strong
           │
           ├─ Optional: 3-day Nono spike (pinned) vs same rows
           │     pass → consider adapter behind flag; keep Docker default until dogfood
           │     fail / semantic mismatch → keep Docker; document why
           │
           └─ srt adapter only with failIfUnavailable hard-required
```

---

## Document history

| Date | Change |
|------|--------|
| 2026-08-07 | Sol plan: keep Docker; Nono spike; pin+recheck; srt failIfUnavailable hard gate |
