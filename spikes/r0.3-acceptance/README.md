# R0.3 — Permit acceptance suite registration

**Status:** Registered. Runner: `./spikes/r0.3-acceptance/run.sh`  
**Registry:** `testdata/permit-suite/registry.json` + `kotro_proxy::permit::suite_registry()`  
**CLI:** `kotro-proxy permit-suite list|check`

## Required cases (this drop)

| ID | Asserts |
|----|---------|
| P-V1A2-ACCEPT | `run --permit` accepts **v1alpha2** only |
| P-V1A1-PERMIT-FIELDS | v1alpha1 + Permit fields rejected |
| P-SIGN-DOMAIN | Cross-version signing-domain substitution rejected |
| P-REPO-MUTATE | Repo identity/pin/base tamper breaks verify |
| P-LAND-NARROW | `draft_pr` → `apply_only` ok; reverse fails |
| P-REPLAY / P-CONCURRENT | One-shot ledger replay + concurrent reserve |
| P-EXPIRY | Expired envelope rejected at prepare |
| P-STAGING | Unsafe out / `../` / nested deny (stage-safety.sh) |
| P-NO-HOST-FALLBACK | No sandbox → agent never runs on host |

Spike evidence rows **P-CONTAIN-4-7** and **P-TOPOLOGY-16-25** cite R0.1a/R0.1b PASS results (re-run with `--spikes`).

## R0.4 CLI (adjacent)

```text
kotro-proxy run --permit <env.json> --trust <trust.json> [--verify-only] -- <agent…>
kotro-proxy receipt verify --trust <store> <receipt>   # fail-closed stub until R3
```

**Exit codes**

| Code | Meaning |
|------|---------|
| **0** | Success (`--verify-only` ok, or future full run) |
| **1** | Failure (verify/ledger/sandbox absent, CLI error) |
| **2** | **Verified but execution unavailable** — gates passed; sandbox launch deferred (R2-A). **Not** CLI misuse. Ledger **unclaimed**. |

Sandbox container launch remains **R2-A**. Claim via `claim_for_sandbox_launch` only when launch is committed.
