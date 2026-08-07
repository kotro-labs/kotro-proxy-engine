# Kotro Permit — task list (**FROZEN**)

**Status (2026-08-07):** **Experimental / code-frozen.**  
**Do not** schedule R2-A expansion, broker expansion, delegation, receipts polish, or Permit adapters.  
**Security and correctness fixes only.** Preserve tests and docs.

**Next direction:** [`KOTRO-CONTROL-LAB.md`](./KOTRO-CONTROL-LAB.md) (vendor-neutral verification suite).

Alpha R2–R4 code that already landed on `main` remains in-tree as reference; it is **not** a mandate to continue. Sunk cost ≠ roadmap.

---

## Freeze rules

| Allowed | Forbidden |
|---------|-----------|
| Bugfixes that keep existing Permit tests green | New broker/land features |
| Doc honesty (experimental banners) | Nono/srt **Permit** adapters |
| Reusing patterns inside Control Lab adapters | Gate A as reason to build more Permit |

Historical design companions (read-only context): positioning, backend contract, sandbox, authority, broker, SOL-REVIEW.

---

## What was built (archive — not a continue list)

| Milestone | State |
|-----------|--------|
| R0 | Complete (v1alpha2, suite, fail-closed run) |
| R2-A / R2-B / R3 / R4 | Landed experimentally; **frozen** — no expansion |
| Gate A Permit recruiting | Superseded by Control Lab external validation gates |

---

## Document history

| Date | Change |
|------|--------|
| 2026-08-07 | **FROZEN** — Control Lab becomes primary OSS bet |
| … | Prior R0–R4 history retained in git |
