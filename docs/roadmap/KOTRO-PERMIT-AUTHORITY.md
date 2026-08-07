# Kotro Permit — signed short-lived authority (PermitSpec → TaskEnvelope)

**Status:** Design for implementation + external LLM review  
**Audience:** Implementers, Sol/Fable/other reviewers  
**Product stance:** Approved direction (v7). Implement after R0.1a containment spike passes.  
**Last updated:** 2026-08-06

**Companions (read as one pack):**  
| Doc | Topic |
|-----|--------|
| [`KOTRO-PERMIT-TASKS.md`](./KOTRO-PERMIT-TASKS.md) | Execution order, gates, acceptance #1–#24 |
| [`KOTRO-PERMIT-SANDBOX.md`](./KOTRO-PERMIT-SANDBOX.md) | Docker vs VM, resources, disclosures, landing UX |
| [`KOTRO-PERMIT-BROKER.md`](./KOTRO-PERMIT-BROKER.md) | Draft-PR broker, run token, allow-once |
| **This file** | How the **permit badge** is created, signed, verified, expired |

---

## 0. One-sentence model

A **short-lived signed permit** is a cryptographically signed **TaskEnvelope**, issued by a key in the operator’s **trust store**, listing what one agent job may do, that becomes invalid after **`expires_at`**.  
`kotro run --permit` refuses to start (and the broker refuses to land) if that badge does not verify.

Analogy: visitor badge — Floor 3 until 5pm, printed by a trusted printer; turnstiles reject after 5pm or if the ink is forged.

---

## 1. Two artifacts (do not conflate)

| Artifact | Audience | Mutable? | Role |
|----------|----------|----------|------|
| **PermitSpec** | Human (YAML) | Yes, before sign | Intent: repo/data, tools, `draft_pr`, TTL, budgets |
| **TaskEnvelope** | Machine | No (re-sign to change) | Canonical authority + signature + timestamps |

```text
PermitSpec.yaml          (human edits)
        │
        │  kotro permit compile / sign
        │  (permit-authority private key)
        ▼
TaskEnvelope             (signed JSON; this is the “permit” at runtime)
        │
        │  kotro run --permit <envelope> -- <agent>
        ▼
verify → sandbox + broker enforce capabilities
```

**Rule:** Agents and `run --permit` consume the **envelope**, not the unsigned spec.  
Inspect UX must label states clearly (from task list):  
`UNSIGNED PERMIT SPEC` | `SIGNED BUT UNVERIFIED ENVELOPE` | `VERIFIED — TRUSTED SIGNER` | `INVALID / EXPIRED`.

---

## 2. What already exists in the codebase (reuse)

Do not reinvent. Wire Permit product onto existing types:

| Piece | Location | Notes |
|-------|----------|--------|
| `TaskEnvelope` schema | `rust/kotro-types/src/envelope.rs` | `api_version`, `kind`, `task_id`, `audience`, `issuer`, times, `capabilities`, `signature` |
| Signing domain | `SIGNING_DOMAIN = KOTRO-TASK-ENVELOPE-V1ALPHA1` | Domain-separated Ed25519 over JCS |
| `sign_envelope` / `verify` | `rust/kotro-types/src/verify.rs` | Signature + trust + time window |
| `TrustStore` | `rust/kotro-types/src/trust.rs` | Explicit trusted public keys |
| MCP `TaskGate` | `rust/kotro-proxy/src/mcp/task_gate.rs` | Optional today; **fail-open** unless `KOTRO_TASK_REQUIRED=true` |
| Expiry fields | `issued_at`, `not_before`, `expires_at` (RFC3339) | Checked in `verify` |

**Permit product change:** for `run --permit`, verification is **mandatory / fail-closed** (not optional MCP-only). Sandbox + broker are additional enforcement planes beyond MCP tool gating.

---

## 3. Envelope fields that matter for Permit

From existing `TaskEnvelope` (illustrative — extend carefully, prefer additive capabilities):

| Field | Purpose |
|-------|---------|
| `task_id` | Stable id for this job |
| `audience` | Who may consume (optional expected-audience check) |
| `issuer` / `principal` | Who issued / for whom |
| `agent_scope` | Which agent names / workload identities |
| `issued_at` / `not_before` / `expires_at` | **Short-lived window** |
| `nonce` | Replay hygiene |
| `capabilities` | tools, models, destinations, credentials, filesystem, budgets |
| `delegation` / `parent` / `depth` | Delegation chains (non-expansion rules in verify) |
| `signature` | Ed25519 over domain \|\| JCS(body) |

### Capabilities Permit will lean on

| Capability area | Permit use |
|-----------------|------------|
| `filesystem` | What project paths may be staged/mounted (not host secrets) |
| `tools` | MCP tool allow list (existing gate) |
| `models` / `destinations` | What Kotro may call upstream |
| `credentials` | Ids/scopes Kotro may use **on host** — never mount secrets into agent |
| `budgets` | max tool calls / spend-like limits |
| **Land (additive for Permit)** | e.g. allow `draft_pr`; **never** default `merge` — see broker doc |

Exact YAML shape for PermitSpec → these fields is **R0.2** work; keep mapping documented there.

---

## 4. What “signed” means

1. Canonicalize envelope body (JCS).  
2. Prepend signing domain bytes.  
3. Sign with **permit-authority** Ed25519 private key.  
4. Attach signature on envelope.  

At verify time Kotro requires **all** of:

| Check | Meaning if fail |
|-------|-----------------|
| Parse OK | Corrupt / wrong kind |
| Signature cryptographically valid | Tampered body or wrong key material |
| Signer pubkey in **trust store** | Random self-signed badge |
| `now` ∈ **`[not_before, expires_at)`** (half-open) | Not yet valid or **expired** |
| `issued_at ≤ not_before < expires_at` | Malformed ordering |
| Audience / non-expansion / kill (as configured) | Policy reject |

**Do not reuse current code unchanged (Sol P1.4):** `issued_at` not checked in `verify::check_time`; trust-key dates use lexical string compare; TaskGate string/`>` vs verify half-open disagree. Permit work must parse timestamps, unify semantics, and test offsets / exact expiry / bad ordering.

**Signed ≠ encrypted.** Contents may be readable; integrity + issuer authenticity are the point.

**Separate keys (invariant):**  
- Permit-authority key — signs envelopes  
- Receipt-mediator key — signs receipts (R3)  
- Never mount private keys into the agent container  

---

## 5. What “short-lived” means

| Mechanism | Effect |
|-----------|--------|
| `expires_at` on envelope | `verify` fails for `now >= expires_at` → no new authority |
| **Container deadline** | `min(expires_at, run_start + max_duration_seconds, operator_ceiling)` → **stop container** so shells cannot keep writing |
| `KOTRO_RUN_TOKEN` TTL | Broker auth dies with run |
| Pre-land revalidation | Permit/token/artifact/approval checked again immediately before push (human wait may cross expiry) |

Recommended alpha defaults (tunable): task window **30–60 minutes**.  

### Replay / “one job” (Sol P1.2 — decide in R0.2)

`nonce` exists on the envelope but is **not** consumed by today’s verifier — do not claim replay hygiene until implemented.

| Mode | Behavior |
|------|----------|
| **One-shot (recommended default)** | Atomically claim `permit_digest`/`nonce`; reject second concurrent/sequential run |
| **Reusable** | `max_runs` / concurrency on envelope; **aggregate** budgets across runs |

Add tests for replay and concurrent start.

---

## 6. Trust store

Anyone can generate a keypair and sign. Trust is operator policy:

```text
TrustStore
  └── list of trusted public keys (permit issuers you accept)
```

| State | Meaning |
|-------|---------|
| Signature valid + signer **not** in trust store | Reject (untrusted issuer) |
| Signature valid + trusted + in time window | Accept |
| Bundled/example pubkey without trust add | **Not** automatic trust (user must add) |

CLI (planned R1): explicit trust add; owner-only perms; atomic writes; no silent overwrite.

---

## 7. Runtime lifecycle (authority plane)

```text
A. Author
   permit init → edit PermitSpec
   permit sign → TaskEnvelope
   permit inspect → show VERIFIED / EXPIRED / etc.

B. Run
   kotro run --permit envelope -- agent…
   1. Load envelope + trust store
   2. verify(...)  → fail closed if anything wrong
   3. Stage ephemeral copy per filesystem capabilities (Option A)
   4. Mint KOTRO_RUN_TOKEN bound to run_id + permit_digest + TTL
   5. Start sandbox; inject broker URL + run token only
   6. Enforce: mounts, deny-all net, MCP/tools per capabilities
   7. On draft PR: broker re-checks run token + permit still allows draft_pr
      + allow-once + artifact hash (broker doc)
   8. After expires_at: authority dead

C. Land / merge
   Kotro opens draft PR (host credentials)
   Human merges on GitHub
```

### Permit vs run token (two layers)

| | Permit (TaskEnvelope) | Run token |
|--|----------------------|-----------|
| Purpose | What the **job** may do | Prove this **container** may call Kotro broker |
| Lifetime | Minutes–hours (task) | Run/session TTL |
| Held by | File on host; verified by Kotro | Injected into agent env |
| Is GitHub token? | No | No |

---

## 8. `permit_digest` and evidence

When product path exists:

- Compute a stable digest of the verified envelope (or canonical signed payload).  
- Attach `permit_digest` + `run_id` to flight-recorder / land audit / (R3) receipts.  
- Success metric (task list): mediated actions and observable denials link back to that digest.

Alpha may stub land logs; R3 ships signed receipts with mediator key + `--trust` verify.

---

## 9. Fail-closed rules (authority)

| Situation | Behavior |
|-----------|----------|
| No `--permit` on `run` | Refuse (Permit CLI) |
| Envelope missing / unreadable | Refuse |
| Signature invalid | Refuse |
| Signer not trusted | Refuse |
| Expired / not-before | Refuse |
| Sandbox backend unavailable | Refuse (no host fallback) |
| Broker called with bad run token | Reject land |
| Permit lacks `draft_pr` | Reject land |
| Allow-once denied | No PR |
| GitHub token missing on host | Clear error; **never** inject into agent |

Contrast today: MCP `TaskGate` can be fail-open if unset — **Permit run path must not copy that**.

### 9b. Enforcement bridge: MCP TaskGate vs `run --permit` (Fable flag)

Two postures coexist for the same envelope type:

| Path | Default posture | Env / flag |
|------|-----------------|------------|
| MCP `mcp-wrap` + TaskGate | **Fail-open** if envelope/trust unset | Unless `KOTRO_TASK_REQUIRED=true` |
| `kotro run --permit` | **Fail-closed** always | `--permit` mandatory; verify or refuse |

**When both are configured for one sealed run:**

1. **`run --permit` fail-closed wins** for starting the sandbox and for broker land.  
2. MCP TaskGate inside that run should be aligned to **enforce** (treat as required) — do not leave MCP fail-open while the outer run claimed a permit.  
3. **Never “unify”** by relaxing `run --permit` to match historical MCP fail-open defaults.  
4. Document this in R0.4 CLI/lifecycle notes and code comments at the `run` entrypoint so a later cleanup does not silently weaken Permit.

Standalone `mcp-wrap` without `run --permit` may keep today’s TaskGate semantics until a separate migration — out of Permit alpha scope, but must not infect Permit.

---

## 10. Normal sandbox vs Permit badge

| | Sandbox alone | Permit (this doc) |
|--|---------------|-------------------|
| Isolation | Process/container | Same + **which** job is authorized |
| Expiry | Optional container TTL | **Cryptographic** `expires_at` |
| Tamper evidence | Weak | Signature breaks on edit |
| Issuer | Implicit “whoever started Docker” | Explicit trusted key |
| Land | Often agent-held git token | Broker + permit capability |

Sandbox without permit = locked room with no badge policy.  
Permit without sandbox = badge theater. **Both required** for `run --permit`.

---

## 11. PermitSpec (human) — expected contents (R0.2 to finalize)

Illustrative checklist for the compiler — not final schema:

- Identity: task name / description  
- Time: TTL or absolute `expires_at`  
- Workspace: source path → ephemeral staging rules (Option A)  
- Forbidden: never mount host secret paths  
- Tools / models allowed  
- Land: `draft_pr: true/false` (default true for happy path; `merge: false` always in default)  
- Budgets: max tool calls, optional resource caps (see sandbox doc)  
- Audience / agent name  

Compiler responsibilities:

- Map to `TaskEnvelope` fields  
- Sign-time path checks only (do **not** claim runtime symlink-to-unmounted-host is enforced by the compiler — mount topology does that)  
- Refuse to emit envelopes that claim host-secret access or merge-by-default  

---

## 12. CLI surface (planned)

| Command | Behavior |
|---------|----------|
| `permit init` | Scaffold PermitSpec |
| `permit inspect` | State labels above; show expiry, capabilities, signer |
| `permit sign` | Spec → signed envelope |
| `permit verify` | verify without running |
| `run --permit <envelope> -- <cmd>` | Verify fail-closed; claim ledger only when sandbox launch committed |
| Exit **2** | **Verified but execution unavailable** (R0.4 deferred launch) — not CLI misuse; ledger unclaimed |
| `--verify-only` | Verify only; ledger unclaimed |

Escape Lab stays `kotro-proxy corpus run` — no collision with top-level `run`.

---

## 13. Implementation checklist (authority)

- [ ] Document PermitSpec ↔ TaskEnvelope field map (R0.2)  
- [ ] `permit init/inspect/sign/verify` with honest inspect states  
- [ ] Trust bootstrap: generate key, add to trust store, owner-only, atomic  
- [ ] `run --permit` always calls `verify` fail-closed  
- [ ] Derive `permit_digest`; thread into run logs / broker  
- [ ] Mint run token bound to digest + run_id (broker doc)  
- [ ] Capability `draft_pr` / deny `merge` in default  
- [ ] Tests: expired, untrusted signer, tampered body, missing trust, happy verify  
- [ ] User docs: badge analogy + “signed ≠ encrypted” + expiry behavior  
- [ ] Do **not** start this before R0.1a containment passes if it delays the kill-shot — parallelize docs/specs only  

---

## 14. Anti-patterns

- Treating unsigned YAML as runtime authority  
- Auto-trusting embedded/example keys  
- Putting permit private key or GitHub token in the agent  
- Extending expiry by editing JSON without re-sign (must fail verify)  
- Claiming compiler path checks replace mount topology  
- Fail-open `run` without permit “for convenience”  
- Equating run token with the permit itself  

---

## 15. Review questions for other LLMs

Please challenge or confirm:

1. Is the two-artifact split (PermitSpec vs TaskEnvelope) clear and necessary?  
2. Is fail-closed verify-on-`run` the right break from today’s optional TaskGate?  
3. Are expiry + trust store + signature sufficient for alpha, with mTLS deferred?  
4. Is binding run token to `permit_digest` the right link to the broker?  
5. Any envelope field gaps for Option A staging + `draft_pr` before R0.2 freezes the map?  
6. Clock skew / expiry edge cases we should specify now?  
7. Does this overclaim vs what `kotro-types` verify actually checks today?

---

## 16. Document history

| Date | Change |
|------|--------|
| 2026-08-06 | Initial: signed short-lived permit lifecycle, reuse of TaskEnvelope, checklists, LLM review prompts |
| 2026-08-06 | Fable: §9b TaskGate vs `run --permit` enforcement bridge |
| 2026-08-06 | Sol: half-open time interval; replay modes; container deadline; do not reuse verify/trust/TaskGate dates unchanged |
