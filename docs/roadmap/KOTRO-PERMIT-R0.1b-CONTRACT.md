# R0.1b — Topology + Option A staging contract

**Status:** Spike **PASS** (2026-08-07) — `spikes/r0.1b-topology/results/`.  
**Source:** Sol P0.2 + Option A inclusion (SOL-REVIEW).

## Network

| Requirement | Test | Result |
|-------------|------|--------|
| Public IP/HTTP blocked | R0.1a #6/#7 | PASS |
| External DNS not exfil | R0.1a #6 | PASS |
| Agent reaches data-plane + mediated upstream | #16 | PASS |
| Host canary content unreachable; gateway exposure recorded | #25 (renamed) | PASS — canary body not leaked; gateway L3 addressability noted |
| Direct upstream off agent net | — | PASS |
| Provider token not in agent | — | PASS |

**Gateway honesty (keep visible — measured exposure, not “Kotro-only”):**  
On Docker Desktop 4.85 / `--internal`, the bridge **gateway IP was TCP-addressable** from the agent (`Connection refused` on the canary port = L3 reachability, no listener). `#25` means **host canary content was unreachable**, not “gateway deny-all.” Do **not** bind secrets on the bridge gateway. Sole-window product claims still need explicit data-plane-only exposure / host firewall — `internal: true` alone is insufficient.

## Option A inclusion

**Tool:** `spikes/r0.1b-topology/stage-repo.sh` (**hardened** — Sol path blockers addressed; safety tests in `test-stage-safety.sh`)

1. Allocate staging only under `$KOTRO_STAGING_ROOT` (`mktemp -d`).  
2. Pin revision → `git archive` tracked files (**no `.git`**).  
3. Optional `--include-untracked` (relative only; nested deny-list; invalid paths fail closed).  
4. Host-side `*.manifest.jsonl` with hashes — **not** mounted into agent.  
5. Preview warns committed deny-pattern files still stage.

## Land (R2-A)

Reviewed artifact → apply to live tree (human). No host git push yet.

## Out of scope here

R2-B broker clean-git push (see BROKER §6b).
