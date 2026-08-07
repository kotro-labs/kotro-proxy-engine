# Backend spike — Nono (planned)

**Do not start** until Gate A wedge signal is in ([`docs/launch/GATE-A-RECRUITING.md`](../../docs/launch/GATE-A-RECRUITING.md)).

Contract: [`docs/roadmap/KOTRO-PERMIT-BACKEND-CONTRACT.md`](../../docs/roadmap/KOTRO-PERMIT-BACKEND-CONTRACT.md).

| Field | Value |
|-------|--------|
| Window | 3 calendar days max |
| Docker | Keep as reference — run same acceptance rows side-by-side |
| Pin | *Fill Nono version + git SHA at spike start* |
| Recheck | 2026-09-07 or +30d from start |
| Must prove | #4/#5 secret reads; #9 fail-closed; note ENOENT vs EACCES mismatch |
| srt note | If exploring Anthropic sandbox-runtime, `failIfUnavailable=true` is mandatory |

## Forbidden claims during spike

- “Codex ships Landlock + seccomp by default” (unverified)
- Replacing Docker as default before pass + product decision
