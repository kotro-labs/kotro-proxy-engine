# Kotro Permit — Sol review contracts (2026-08-06)

**Status:** Design corrections required. **Do not freeze R0.2 or begin R2** until these are settled in the pack and reflected in specs.  
**Sol:** Direction strong; no file changes / no spike run by Sol.  
**Folded into:** TASKS, SANDBOX, BROKER, AUTHORITY, README + spike `run.sh` DNS fix.

---

## Verdict

| Gate | Sol ruling |
|------|------------|
| Direction | Strong — keep R2-A/R2-B, fail-closed run, PermitSpec/envelope, Option A honesty, anti-fatigue allow-once |
| Freeze R0.2 / start R2 | **No** — settle contracts first |
| R0.1a | May **run** now, but **do not declare pass / Gate A evidence** until #6 DNS assertion is correct |
| Before R2 | Five security contracts below (+ Option A inclusion policy in R0.1b) |

---

## P0 — fix before accepting R0.1a / claiming “only window”

### P0.1 External DNS must not falsely pass (#6)

**Issue:** Spike treated #6 as PASS when only HTTP failed; successful external DNS still possible via Docker’s embedded resolver on `--internal` networks.

**Contract:**
- #6 **FAIL** if external DNS resolution returns usable answers (exfil channel).
- #6 **FAIL** if external HTTP succeeds.
- #6 **PASS** only if both external DNS and external HTTP fail to provide a channel.
- Prefer a **unique controlled query** observable outside the container for Gate A evidence.
- Spike `run.sh` updated accordingly — re-run required; empty/`old` results are not evidence.

### P0.2 `internal: true` ≠ “Kotro data-plane only”

**Issue:** Containers on an internal bridge can still reach gateway IP and appropriately configured host services.

**Contract (R0.1b):**
- Prove agent **cannot** reach arbitrary host/gateway services (not only public internet).
- Add **host canary** service + **gateway scan** tests.
- If claim is “only window out = Kotro,” **host firewall / service binding rules are in scope** — cannot defer firewall forever while making that claim.
- Doc language: `internal: true` = deny public egress baseline; **not** full dual-home proof.

### P0.3 Draft-PR must not trust agent-controlled `.git`

**Issue:** Broker accepting `staging_commit` then running host Git in an agent-writable repo enables malicious `.git/config`, hooks, helpers, refs, post-approval worktree swaps; `.git` changes omit from ordinary diffs.

**Contract (before R2-B / any host git push):**
1. **Never** consume agent-controlled `.git` metadata.  
2. Materialize the **approved artifact** into a **clean, host-owned** repository.  
3. Disable hooks and inherited Git configuration on that host repo.  
4. Push an **exact immutable tree/commit**, not “current worktree” or agent-chosen mutable branch tip alone.  
5. **Revalidate hash immediately before** the side effect, **after** allow-once (and after any human wait).

---

## P1 — settle before R2 (authority / broker / time)

### P1.1 Repository / remote / ref authority bound to permit

**Contract (R0.2):**
- Permit/run bound to **canonical repository identity** + **allowed base** (ref + optionally pinned revision).  
- Kotro **generates** head branch name; **ignore** agent-supplied remotes/refspecs.  
- Agent-supplied `head_branch` / `base_branch` in request sketch are **not** authoritative.  
- **Disclose:** push / draft PR may immediately trigger **CI, bots, `pull_request` integrations** — “draft / no merge” ≠ “no execution.”

### P1.2 One job / budgets vs replay

**Contract (choose explicitly in R0.2; implement before R2):**

| Mode | Behavior |
|------|----------|
| **One-shot (default recommendation)** | Atomically claim `permit_digest`/`nonce`; second `run --permit` rejects |
| **Reusable** | Envelope carries `max_runs` / concurrency; **aggregate** budgets across runs |

Add acceptance tests: replay + concurrent start.  
Today’s verifier does **not** consume `nonce` — do not claim replay hygiene until implemented.

### P1.3 Expiry must terminate filesystem authority

**Contract:**
- Container deadline = `min(envelope.expires_at, run_start + max_duration_seconds, operator_ceiling)`.  
- At deadline: **stop container** (running shell must not keep writing).  
- Before push/PR: revalidate permit, run token, artifact, approval — especially if human allow-once waited past expiry.

### P1.4 Time verification — do not reuse current code unchanged

**Facts (Sol-verified):**
- `issued_at` not validated in `verify.rs` `check_time`.  
- Trust-key dates use **lexical** string compare in `trust.rs` (wrong across RFC3339 offsets).  
- TaskGate uses string `>` and allows exact expiry instant; `verify` uses `now >= expires_at` (half-open).  
- AUTHORITY previously said closed `[not_before, expires_at]` — wrong vs verifier.

**Contract:**
- Parse all timestamps to instants.  
- Validity interval: **`[not_before, expires_at)`** (half-open).  
- Require `issued_at ≤ not_before < expires_at`.  
- Unify TaskGate with verify semantics.  
- Tests: offsets, exact expiry, malformed ordering.

### P1.5 Option A inclusion policy (R0.1b, before R2)

“Copy the repo” is insufficient.

**Safe default (Sol):**
- Tracked files at a **pinned revision**, plus  
- Explicit, **previewed** selection of untracked changes.  
- Review UI shows **exactly** what data enters the sandbox.  
- Do not silently copy full tree (`.env`, sockets, submodule metadata, `.git`) or silently drop legitimate WIP.

---

## Doc-only corrections (must apply)

| Item | Fix |
|------|-----|
| Suite references | Say **#1–#24** (not #1–#20) where broker rows exist |
| “Token only in proxy” | **Provider/GitHub** tokens outside agent; **`KOTRO_RUN_TOKEN` intentionally enters** agent |
| Sandbox PR wording | Kotro opens draft PR **once R2-B is available**; R2-A is apply-only |
| `draft_pr` on envelope | `deny_unknown_fields` → R0.2 must choose **schema-version bump** or map land via **existing capability form** |

---

## What Sol said looks solid (preserve)

- R2-A thesis vs R2-B positioning  
- Fail-closed `run --permit` vs legacy TaskGate  
- PermitSpec vs signed TaskEnvelope  
- Secret placement, Option A, Docker/microVM honesty  
- Allow-once anti-fatigue + hash invalidation  
- Existing proxy vs control/telemetry listeners as starting point for plane separation  

---

## Next action

1. Keep next engineering action = **R0.1a** with corrected DNS (#6).  
2. Do **not** treat a pre-fix spike log as Gate A evidence.  
3. Do **not** freeze R0.2 or start R2 until P0.2–P0.3 and P1.* contracts are in R0.1b/R0.2 specs.  
4. No product implementation in this fold — docs + spike harness assertion only.
