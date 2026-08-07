# Kotro Permit — implementation task list (v7.1)

**Status:** R0 complete. **R2-A complete** (sandbox + Option A + apply + dual-home dataplane + dogfood). Gate A recruiting ∥.  
**Next:** R3 harden (attenuation, signed land receipts, Escape Lab broker rows). R2-B dogfood: `spikes/r2b-broker/`.
**Runtime:** Docker Desktop **≥ 4.x, native arch** (SANDBOX §6.3).

**Fable (2026-08-06):** Pack verified against source; R2-A/R2-B + anti-fatigue + TaskGate bridge folded.  
**Sol (2026-08-06):** P0/P1 contracts in SOL-REVIEW; DNS+#25 spikes executed.

**Companions:**  
- [`KOTRO-PERMIT-README.md`](./KOTRO-PERMIT-README.md) — index  
- [`KOTRO-PERMIT-SOL-REVIEW.md`](./KOTRO-PERMIT-SOL-REVIEW.md) — **Sol contracts (blocking R0.2 freeze / R2 start)**  
- [`KOTRO-PERMIT-SANDBOX.md`](./KOTRO-PERMIT-SANDBOX.md)  
- [`KOTRO-PERMIT-AUTHORITY.md`](./KOTRO-PERMIT-AUTHORITY.md)  
- [`KOTRO-PERMIT-BROKER.md`](./KOTRO-PERMIT-BROKER.md)  

**History:** … → v7 → Fable flags → **v7.1 Sol contracts** (DNS, dual-home honesty, clean-git land, replay, time, inclusion).

**Sol / Fable invariants (unchanged core):** Option A; narrow symlink; containment spike label; suite enumerated; ENOENT-prefer; R2-A then R2-B; run fail-closed vs TaskGate; anti-fatigue allow-once.

### Alpha scope discipline (v6 → v7 cost)

| Stage | Deliver | Proves | Gate |
|-------|---------|--------|------|
| **R2-A (thesis)** | R2.1–R2.3: sandbox + Option A inclusion policy + dual-home **with host canary** + **reviewed diff → apply** | Containment + permit land without GitHub | Gate B *partial* OK |
| **R2-B (positioning)** | R2.4–R2.5: thin broker on **host-owned clean git** + allow-once + permit-bound repo/base | Speed without token-in-agent | After R2-A; after Sol P0.3/P1.1 |

**Do not freeze R0.2 schema until:** land capability representation chosen (version bump vs existing form); one-shot vs reusable permit; time interval `[not_before, expires_at)` specified for implementers.
---

## Product

**Sentence (alpha):**  
*Run Claude Code in an isolated workspace under a signed, short-lived permit defining which project data, tools, and external services it may access — then land via a Kotro-brokered draft PR you confirm.*

Runtime image = **trusted execution material** (outside the user-data capability set).

| **Milestone** | **Claim** |
|-----------|--------|
| **Alpha R2-A** | Secret read denied; ephemeral copy; **reviewed diff → apply**; no secrets in agent |
| **Alpha R2-B** | Same + **thin broker → allow-once draft PR**; you merge |
| **R3** | Hardened broker (attenuation, signed land receipts), Escape Lab authority rows |
| **Launch rule** | Do not claim R2-B/R3 before implemented; R2-A is enough to prove thesis |

**Validation gates:**  
- **Gate A (before significant R1):** ≥3 users **commit to trying** — recruit with R0.1a containment clip.  
- **Gate B (before R3 harden):** ≥3 users **completed** an alpha run (ideally including a real draft PR).

**Positioning:**  
*Like other coding agents we open a draft PR for you — unlike them, the agent never holds your GitHub token; Kotro does, after you confirm.*

---

## Critical invariants

1. **`kotro-proxy run --permit` is fail-closed** (`--permit` mandatory).  
   Greenfield enforce; refuse without sandbox; proxy death → fail closed. Under `internal: true`, proxy death is topology-proven fail-closed.

2. **Wrapper ≠ OS confinement.** Proof = `#4`/`#5`.

3. **Dual-homed network:** Agent → Kotro **data-plane** only; Kotro → upstream/GitHub; no control-plane / docker.sock to agent.  
   `internal: true` = **public egress baseline only** — **not** proof of “Kotro only.” R0.1b must add **host canary + gateway scan**; host firewall/bind rules are in scope if we claim sole window (see SOL-REVIEW P0.2).

4. **Host write boundary — Option A locked** with explicit **inclusion policy** (tracked@pinned + previewed untracked — not naive full-tree copy). Land: R2-A apply; R2-B broker on **host-owned clean repo** (never agent `.git`).  
   `--live-workspace` later = lower security.

5. **Mount topology** for host secrets. Narrow symlink claim (see sandbox doc).

6. **Receipts (R3 full):** mediator signer; `receipt verify --trust`; distinguish signature-valid / signer-trusted / permit-trusted / chain-complete; `permit_digest` on events. Alpha may stub land audit logs.

7. **PermitSpec → TaskEnvelope** (two artifacts). Land capability: schema-version bump **or** existing capability form (R0.2 choose). Default at most draft land — **not** `merge`.

8. **Keys:** separate permit vs receipt identities; never mount private keys or **provider/GitHub tokens** into agent; owner-only; atomic writes.

9. **Broker auth:** `KOTRO_RUN_TOKEN` **intentionally** enters agent; provider/GitHub tokens stay on host. See broker doc + SOL-REVIEW P0.3/P1.1.

10. **Replay:** one-shot claim of nonce/digest **or** reusable `max_runs` + aggregate budgets (decide in R0.2; add tests).

11. **Time:** validity **`[not_before, expires_at)`**; `issued_at ≤ not_before < expires_at`; container deadline `min(expires_at, start+max_duration, ceiling)` stops the sandbox; revalidate before push. Do **not** reuse current verify/TaskGate/trust date logic unchanged (SOL-REVIEW P1.4).

---

## Execution order

### R0 — Spike, then specs

| ID | Task | Done when |
|----|------|-----------|
| **R0.1a** | **Containment feasibility spike** | **PASS** 2026-08-07 — #4–#7 incl. DNS |
| **R0.1b** | Topology + staging contract | **PASS** topology spike; `stage-repo.sh`; gateway L3 note |
| **R0.2** | PermitSpec → TaskEnvelope | **v1alpha2** + signed `repository`/`land`; one-shot ledger — see [`KOTRO-PERMIT-R0.2-MAPPING.md`](./KOTRO-PERMIT-R0.2-MAPPING.md) |
| **R0.3** | Acceptance suite registered | **Done** — `testdata/permit-suite/registry.json`, `permit::suite`, `spikes/r0.3-acceptance/run.sh` |
| **R0.4** | CLI lifecycle | **Done (gates)** — `run --permit` v1alpha2; exit **2** = verified but execution unavailable (unclaimed); `claim_for_sandbox_launch` for R2-A; atomic `.claim` files (no sticky lockdir); `receipt verify` stub |
### R1 — After Gate A

| ID | Task |
|----|------|
| **R1.1–R1.5** | Compiler, keys/trust, permit CLI inspect states, MCP enforce, fail-closed startup (as v6) |

### R2 — Alpha (staged)

| ID | Task | Done when |
|----|------|-----------|
| **R2.1** | `run --permit` + mandatory sandbox | **Done** — Docker `--internal`, claim-on-launch, no host fallback |
| **R2.2** | Ephemeral copy + review artifact + **apply** | **Done** — Option A stage + `.review.diff` + `kotro-proxy apply` |
| **R2.3** | Dual-homed + host canary posture; LLM/GitHub creds only on host | **Done** — agent/up nets + dataplane sidecar; `KOTRO_RUN_TOKEN` + dataplane URL in agent; provider tokens on dataplane/host only |
| **R2.3b** | Dogfood R2-A → Gate B *partial* | **Done** — `spikes/r2a-dogfood/run.sh` |
| **R2.4** | Mint/inject `KOTRO_RUN_TOKEN`; broker URL on data-plane | **Done** — token mint/verify; dataplane `/v1/broker/*` forward + `--broker-forward` |
| **R2.5** | Thin broker: clean host git + permit-bound refs + allow-once anti-fatigue + revalidate-before-push | **Done** — `permit::broker` + `broker draft-pr\|serve`; #21–#24 unit |
| **R2.6** | Golden path with draft PR when R2-B ready | **Partial** — dry-run dogfood; live `gh pr create --draft` needs host `GITHUB_TOKEN` |
| **R2.7** | Dogfood → Gate B full | **Partial** — `spikes/r2b-broker/` dry-run; live PR not claimed |

### R3 — Harden broker → R4

| ID | Task |
|----|------|
| **R3.1+** | Attenuation, signed land receipts, Escape Lab broker abuse rows, polish |
| **R4.*** | Demo/README/launch with honest claims |

---

## Acceptance suite (#1–#24 + Sol adds)

Fake `$HOME` + fake SSH key only.

| # | Test | Notes |
|---|------|--------|
| **1–5** | Path / symlink / shell / Python secret reads | As before; R0.1a for #4/#5 |
| **6** | Hostile-domain: **external DNS and HTTP** both fail as channels | R0.1a; DNS success = FAIL |
| **7** | Hostile-IP egress | R0.1a |
| **8–9** | Redirect defer; sandbox unavailable fail-closed | |
| **10** | Provider **and GitHub** tokens absent from agent; `KOTRO_RUN_TOKEN` may be present | Clarify “token only in proxy” |
| **11–13** | Tamper / trust | |
| **14–15** | Live repo / land path (apply and/or clean-git draft PR) | |
| **16–18** | Dual-home reachability; no control-plane; no docker.sock | |
| **19–20** | Attacker receipt; proxy death | |
| **21–24** | Forged run token; artifact mismatch; allow-once deny; no merge | Broker |
| **25** | Host canary content unreachable; gateway exposure recorded | Not “gateway denied”; L3 addressability may exist |
| **26** | Replay / concurrent second run (one-shot ledger) | R0.2/R2 |
| **27** | Expiry stops container FS writes; push rejected after expiry | P1.3 |
| **28** | Staging: reject unsafe out / `../` extras / nested denied / host manifest integrity | R0.3 |

**Success metric:** Mediated actions and observable denials linked to permit digest + run ID.

---

## Gate A recruiting asset (from R0.1a)

**~60s clip** labeled **"Containment feasibility spike"** (not Permit/receipt theater):  
poisoned README → `~/.ssh` → denied (ENOENT / FileNotFound).

---

## Start sequence

```
R0.1a (DNS-fixed #6) ──hard stop / no false pass──►
  R0.1b (canary + inclusion) ∥ R0.2 contracts (do not freeze early) → R0.3–R0.4 →
  Gate A → R1 → R2-A → Gate B partial → R2-B (clean-git broker) → R3 → R4
```

**Next:** Gate A recruiting ∥ R3. R2-A/R2-B alpha land paths exist (apply + thin broker dry-run).
