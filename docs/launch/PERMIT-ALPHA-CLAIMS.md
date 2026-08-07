# Kotro Permit — alpha claims (R4)

**Status:** Alpha engineering complete through R3. This page is the **honesty contract** for demos, README, and recruiting.  
**Date:** 2026-08-07

If a claim is not listed under **You may claim**, do not say it in a launch post.

**Related:** [`../roadmap/KOTRO-PERMIT-POSITIONING.md`](../roadmap/KOTRO-PERMIT-POSITIONING.md) · [Gate A](./GATE-A-RECRUITING.md)

---

## Positioning (cooperative)

> **Nono, sandbox-runtime (srt), or Docker enforce the boundary. Kotro makes the job authority portable, delegable, and provable.**

**Remaining wedge** (not “we uniquely invented signed permits” — OAP already has signed passports/decisions):

- Implemented delegation / attenuation  
- Enforcement outside an instrumented hook  
- Source-pin / base-SHA binding  
- Receipt spanning sandbox + mediation + workspace + landing  

---

## One-sentence product (safe)

*Run a coding agent in a Docker sandbox under a signed, short-lived permit on an ephemeral repo copy — land by reviewed apply, or (with host GitHub credentials) a Kotro-brokered draft PR you confirm; the agent never holds your provider or GitHub tokens.*

---

## You may claim

| Area | Claim | Evidence |
|------|--------|----------|
| **Fail-closed run** | `kotro-proxy run --permit` refuses without a verified v1alpha2 envelope + trust; no host-agent fallback | Unit + CLI gates |
| **Containment spike** | Host secrets not mounted → shell `ENOENT` / Python `FileNotFoundError` | `spikes/r0.1a-containment/` **PASS** |
| **Option A land (R2-A)** | Ephemeral stage → `*.review.diff` → `kotro-proxy apply` | `spikes/r2a-dogfood/` |
| **Dual-home dataplane** | Agent reaches Kotro dataplane; provider token stays on dataplane/host | R2.3 dogfood |
| **Thin broker (R2-B)** | Host-owned clean git + allow-once + run-token + artifact bind; dry-run path works | `spikes/r2b-broker/` **DOGFOOD_OK** |
| **Receipts (R3)** | Mediator-signed land receipt; `receipt verify --trust` distinguishes signature / trust / `permit_digest` / chain | Unit + dogfood `RECEIPT_OK` |
| **Token attenuation (R3)** | `KOTRO_RUN_TOKEN` is `draft_pr`-scoped, TTL-bound, one-shot for land | Unit |
| **Credentials** | Agent may hold `KOTRO_RUN_TOKEN` only — not `GITHUB_TOKEN` / provider keys | Docker env deny list + dogfood |

**Positioning line (broker):**  
*Like other coding agents we can open a draft PR for you — unlike them, the agent never holds your GitHub token; Kotro does, after you confirm.*

---

## You must not claim (yet / ever as Docker-alpha)

| Overclaim | Reality |
|-----------|---------|
| “Nobody else has signed portable authority / receipts” | **OAP** (and peers) already do — cite as validation; our wedge is narrower (see positioning) |
| “Hypervisor / VM isolation” | Docker shares a kernel — workspace + network confinement only |
| “Kotro-only network” / “no path to the host” | `internal: true` = public egress baseline; gateway L3 addressability is a **measured** note (R0.1b #25) |
| “Live draft PR always works out of the box” | Live `gh pr create --draft` needs host `GITHUB_TOKEN`/`GH_TOKEN`; dogfood is **dry-run** |
| “Gate B complete (≥3 users finished alpha runs)” | Recruiting/parallel — do not invent user counts |
| “Containment clip proves the product / unique signed authority” | Containment is **table stakes**; Gate A asks wedge questions first |
| “Codex ships Landlock + seccomp by default” | **Unverified** — do not repeat without primary docs |
| “Draft PR means no CI / no bots” | Draft ≠ no execution — disclose CI/`pull_request` apps |
| “We merge for you” / “merge scope in default permit” | Human merges; `merge` scope forbidden in alpha broker |
| “Confluence / Figma / Notion updated under Permit” | Out of scope |
| Escape Lab “14/14 attacks prevented” | Match **declared** behavior; known `none` rows stay documented |
| “We replace vendor sandboxes” | Cooperative: they enforce the boundary; we issue the job badge |

---

## Limits & trust boundaries (user-facing)

Full copy: [`PERMIT-LIMITS.md`](./PERMIT-LIMITS.md).

Short version:

- Sandbox = **Docker** (not microVM).
- Live repo is **not** mounted RW; Option A staging + inclusion policy.
- Model / GitHub credentials stay on the **host / dataplane**.
- If Docker is unavailable, Permit **refuses** (exit semantics: verified-but-unavailable = **2** for `--prepare-only`).
- Resource caps (memory/CPU/pids) apply; OOM kills are intentional protection.

---

## Demo paths (pick one)

| Goal | Command / doc |
|------|----------------|
| Containment (~60s honesty only) | [`GATE-A-RECRUITING.md`](./GATE-A-RECRUITING.md) — **after** wedge questions |
| Thesis land (apply) | `spikes/r2a-dogfood/run.sh` |
| Broker + receipt dry-run | `spikes/r2b-broker/run.sh` |
| Golden path narrative | [`PERMIT-GOLDEN-PATH.md`](./PERMIT-GOLDEN-PATH.md) |
| Backend / Nono spike rules | [`../roadmap/KOTRO-PERMIT-BACKEND-CONTRACT.md`](../roadmap/KOTRO-PERMIT-BACKEND-CONTRACT.md) |

---

## Roadmap pack

Design detail: [`../roadmap/KOTRO-PERMIT-README.md`](../roadmap/KOTRO-PERMIT-README.md).
