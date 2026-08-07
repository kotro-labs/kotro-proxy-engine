# Kotro Permit — design pack (review before implementation)

**Purpose:** Index for humans / other LLMs before coding beyond the containment spike.  
**Sol (2026-08-06):** Direction strong — **do not freeze R0.2 or begin R2** from the pre-contract pack.  
**Next engineering step:** **Gate A wedge validation** (Sol) — not more Permit code. Positioning: [`KOTRO-PERMIT-POSITIONING.md`](./KOTRO-PERMIT-POSITIONING.md). Backend: [`KOTRO-PERMIT-BACKEND-CONTRACT.md`](./KOTRO-PERMIT-BACKEND-CONTRACT.md). Claims: [`../launch/PERMIT-ALPHA-CLAIMS.md`](../launch/PERMIT-ALPHA-CLAIMS.md).
**Date:** 2026-08-07

---

## Read order

1. [`KOTRO-PERMIT-POSITIONING.md`](./KOTRO-PERMIT-POSITIONING.md) — **Sol: OAP honesty + cooperative wedge**  
2. [`../launch/GATE-A-RECRUITING.md`](../launch/GATE-A-RECRUITING.md) — **urgent: wedge questions, not containment lead**  
3. [`KOTRO-PERMIT-BACKEND-CONTRACT.md`](./KOTRO-PERMIT-BACKEND-CONTRACT.md) — Docker reference; Nono spike; srt failIfUnavailable  
4. [`KOTRO-PERMIT-SOL-REVIEW.md`](./KOTRO-PERMIT-SOL-REVIEW.md) — Sol P0/P1 contracts (blocking)  
5. [`KOTRO-PERMIT-TASKS.md`](./KOTRO-PERMIT-TASKS.md) — v7.1 order, suite #1–#27  
6. [`KOTRO-PERMIT-SANDBOX.md`](./KOTRO-PERMIT-SANDBOX.md)  
7. [`KOTRO-PERMIT-AUTHORITY.md`](./KOTRO-PERMIT-AUTHORITY.md)  
8. [`KOTRO-PERMIT-BROKER.md`](./KOTRO-PERMIT-BROKER.md)  

Spike: `spikes/r0.1a-containment/` (**PASS**)  
R0.1b: `spikes/r0.1b-topology/` (**PASS**) + [`KOTRO-PERMIT-R0.1b-CONTRACT.md`](./KOTRO-PERMIT-R0.1b-CONTRACT.md)  
R0.2 draft: [`KOTRO-PERMIT-R0.2-MAPPING.md`](./KOTRO-PERMIT-R0.2-MAPPING.md) (**v1alpha2** landed)  
R0.3/R0.4: [`spikes/r0.3-acceptance/`](../../spikes/r0.3-acceptance/) + `kotro-proxy run --permit`  
R2-A: `run --permit --repo … -- <agent>` → stage + Docker + `*.review.diff` → `kotro-proxy apply --repo … --diff …`  
R2-B: `kotro-proxy broker draft-pr|serve` — clean host git + allow-once + run token; dogfood `spikes/r2b-broker/`  
R3: signed land receipts (`receipt verify --trust`) + attenuated one-shot land tokens + broker rate limit  
**R4 launch:** [`../launch/PERMIT-ALPHA-CLAIMS.md`](../launch/PERMIT-ALPHA-CLAIMS.md) · [`../launch/PERMIT-LIMITS.md`](../launch/PERMIT-LIMITS.md) · [`../launch/PERMIT-GOLDEN-PATH.md`](../launch/PERMIT-GOLDEN-PATH.md)  
Gate A: [`../launch/GATE-A-RECRUITING.md`](../launch/GATE-A-RECRUITING.md)  
**Runtime prerequisite:** Docker Desktop **≥ 4.x, native arch** (arm64 on Apple Silicon) — avoid leftover Intel/HyperKit installs.

---

## Elevator summary

| Idea | Choice |
|------|--------|
| Wedge | Portable / delegable / provable **job authority** on top of Nono/srt/Docker (not containment alone) |
| Sandbox | Docker **reference**; Option A; `internal: true` = public baseline only; Nono = time-boxed spike only |
| Badge | Signed TaskEnvelope; `[not_before, expires_at)`; replay mode explicit |
| Land R2-A | Reviewed diff → apply |
| Land R2-B | Broker + **clean host git** + permit-bound repo/base + allow-once |
| Hard stops | Do not overclaim vs OAP; Gate A = wedge questions; no unverified Codex Landlock defaults |

---

## Review fold-ins

**Fable:** R2-A/R2-B scope; allow-once anti-fatigue; TaskGate vs run bridge.  

**Sol:**  
1. #6 DNS+HTTP (no false PASS)  
2. Host canary / gateway — `internal: true` ≠ Kotro-only  
3. Never trust agent `.git`; clean host materialize; revalidate before push  
4. Permit-bound repo/base; Kotro-generated head; CI disclosure  
5. One-shot vs reusable + aggregate budgets  
6. Expiry stops container; align TTL to `expires_at`  
7. Rewrite time verification (don’t reuse current verify/trust/TaskGate as-is)  
8. Option A inclusion policy  
9. Doc nits: suite #1–#24+, token wording, R2-A PR claim, schema bump for land capability  

---

## Non-negotiables (short)

- Fail-closed `run --permit`  
- Option A + inclusion preview  
- No provider/GitHub tokens in agent (`KOTRO_RUN_TOKEN` OK)  
- Draft ≠ no CI execution  
- No broker / R2 until contracts + R0.1a honest pass  

---

## Document history

| Date | Change |
|------|--------|
| 2026-08-06 | Index pack |
| 2026-08-06 | Fable flags |
| 2026-08-06 | Sol contracts index → SOL-REVIEW; v7.1; DNS harness fix noted |
| 2026-08-07 | R2-B thin broker landed; next = R3 |
| 2026-08-07 | R3: land receipts + token attenuation + rate limit |
| 2026-08-07 | R4: PERMIT-ALPHA-CLAIMS + limits + golden path + README |
| 2026-08-07 | Sol: positioning vs OAP; Gate A wedge-first; backend contract (Docker+Nono spike) |
