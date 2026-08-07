# R0.2 — PermitSpec → TaskEnvelope mapping

**Status:** Sol-ruled direction (2026-08-07). **v1alpha2 types landed** in `rust/kotro-types`
(`RepositoryAuthority`, `LandAuthority`, signing domain `KOTRO-TASK-ENVELOPE-V1ALPHA2`).  
Do **not** start R0.3/R0.4 product wiring until staging path stays hardened and CLI/suite target this shape (not v1alpha1).

**Depends on:** R0.1a PASS, R0.1b topology PASS (gateway honesty noted).

---

## Sol rulings (accepted)

| Topic | Ruling |
|-------|--------|
| Schema | Prefer **`kotro.dev/v1alpha2` now**. Old parsers rejecting unknown version = safe fail-closed. |
| Land encoding | Do **not** freeze “credentials and/or destinations.” In v1alpha2: **`destinations`** authorize broker **route** access; **`credentials`** authorize host mediator **GitHub operation**. Keep scopes separate. |
| Repository authority | **Inside signed envelope**: repository identity, source pin, base_ref, **base_sha**, land mode. **No sidecar as authority.** Do not overload `audience`. |
| Resolve timing | Resolve remote identity + **base_sha at sign time** (not “sign/run time”). Alpha: **require `base_sha`**; fail closed if base moves — user issues a new short-lived permit. |
| One-shot | **Confirmed default.** Ledger: `unused → reserved → consumed` keyed by **`permit_digest`** (digest already includes nonce). Define concurrent-start + whether Docker pre-agent failure **releases** reservation. |
| v1alpha1 fallback | Only if delayed: **one** canonical encoding (not and/or), exact scope vocabulary, non-expansion tests — still prefer v1alpha2. |

---

## v1alpha2 sketch (signed body)

```text
api_version: kotro.dev/v1alpha2
kind: TaskEnvelope
# … existing identity/time/nonce/depth/parent/delegation/signature …

repository:                    # NEW — signed authority
  identity: "github.com/org/repo"   # canonical remote identity at sign time
  source_pin: "<full sha>"          # tree staged into sandbox
  base_ref: "main"
  base_sha: "<full sha>"            # required (alpha); fail if moved at run
  # head_branch NOT agent-supplied — generated at run, may be recorded in receipt

land:                          # NEW — signed authority
  mode: draft_pr | apply_only
  # merge never in default compiler output

capabilities:
  filesystem: [{ root: "/workspace", operations: ["read","write"] }]
  destinations:                # broker route allow
    - { scheme: "kotro", host: "broker", port: 8080, path_prefix: "/v1/broker/" }
  credentials:                 # host mediator GitHub op (only if land.mode=draft_pr)
    - { id: "github", scopes: ["draft_pr"] }   # never "merge" by default
  budgets: { … }
```

**Signing domain:** new domain string for v1alpha2 (do not reuse v1alpha1 domain).  
**Non-expansion:** child cannot widen repository identity, base_sha, land.mode, destinations, or credential scopes.

---

## PermitSpec sketch (human → compiler)

```yaml
api_version: kotro.dev/permit-spec/v0
kind: PermitSpec
metadata:
  name: fix-docs-typo
spec:
  ttl_seconds: 3600
  one_shot: true
  agent:
    names: ["claude-code"]
  repository:
    path: "/Users/me/proj"     # host path for staging source only (not authority alone)
    # At SIGN time compiler resolves:
    #   identity, source_pin, base_ref, base_sha (required)
    base_ref: "main"
  staging:
    include_untracked: []      # explicit; previewed
  land:
    mode: apply_only           # R2-A default; draft_pr for R2-B
  budgets:
    max_tool_calls: 200
    max_duration_seconds: 3600
```

Compiler **fails closed** if `base_sha` cannot be resolved at sign time.

---

## One-shot ledger (atomic)

```text
unused  --claim_for_sandbox_launch(run_id)-->  reserved  --agent_started-->  consumed
                \--release_on_pre_agent_failure--> unused   (Docker/sandbox fail before agent PID)
```

- Ledger key: **`permit_digest`**  
- Storage: **one `{digest}.claim` file** created with `create_new` — **no shared lockdir** (crash cannot leave a sticky lock blocking the ledger)  
- Concurrent `reserve` on same digest: exactly one wins; loser fails closed  
- After `consumed`, further claims rejected until new envelope  
- **R0.4 / R2-A handoff:** `--verify-only` and “verified but execution unavailable” (exit **2**) must **not** claim; claim only when sandbox launch is committed  
- Acceptance: #26 replay + concurrent-start

---

## Staging (Option A) — product requirements

Spike script hardened for path safety; still promote carefully into product.

| Requirement | Rule |
|-------------|------|
| Output root | Only under Kotro-owned staging root (`$KOTRO_STAGING_ROOT` / `~/.kotro/staging`); `mktemp -d` therein |
| No `rm -rf` on caller paths | Never delete arbitrary `--out`; create new dir only |
| Extras paths | Reject `..`, absolute paths; normalize; stay under repo |
| Nested denies | Walk directories; skip deny-listed names at every level |
| Manifest | **Host-side** only (not in agent tree); normalized paths, types, content hashes; no host path leak into agent mount |
| Preview | Warn that **tracked/committed** deny-pattern files are still staged |
| Worktrees | Detect via `git rev-parse --is-inside-work-tree`, not `test -d .git` |

R0.3 must register tests: unsafe out paths, `../` extras, nested denied files, manifest tampering.

---

## R0.1b qualification (Sol)

Evidence supports dual-home shape + canary content unreachable.  
**Rename #25:** not “gateway reachability denied” → **“host canary content unreachable; gateway exposure recorded.”**

---

## Before R0.3 / R0.4

1. ~~Land v1alpha2 types + signing domain + non-expansion tests~~ (done in `kotro-types`).
2. Keep staging hardened script; add R0.3 safety tests.  
3. Gate A recruiting can proceed **independently** (R0.1a clip).
