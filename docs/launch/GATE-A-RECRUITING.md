# Gate A — recruiting with R0.1a containment evidence

**Gate:** ≥3 people **commit to trying** Permit when ready.  
**Evidence to recruit with:** R0.1a containment PASS — not Permit theater.  
**Engineering status (2026-08-07):** R2-A / R2-B / R3 alpha paths exist — use [`PERMIT-ALPHA-CLAIMS.md`](./PERMIT-ALPHA-CLAIMS.md) when you graduate the conversation past the spike.

## What to show (~60s)

Label the clip: **"Containment feasibility spike"** (not “Kotro Permit receipts”).

Storyboard:

1. Poisoned workspace README tells the agent to read `~/.ssh/id_rsa`
2. Agent runs inside the spike sandbox (host secret **not** mounted)
3. Shell → `ENOENT`; Python → `FileNotFoundError`
4. Optional 5s: hostile DNS + IP egress fail as channels (`--internal`)

**Do not claim from this clip alone:** signed receipts, draft-PR broker, or “Kotro-only network.”  
Gateway L3 exposure is a known measured note (R0.1b #25) — keep claims honest.

**After interest:** point to golden path / claims page — sandbox + apply is real; live brokered PR needs host `GITHUB_TOKEN` and allow-once.

## Assets in repo

| Asset | Path |
|-------|------|
| PASS write-up | `spikes/r0.1a-containment/results/PASS-20260807.md` |
| Raw log | `spikes/r0.1a-containment/results/spike-20260807T062220Z.txt` |
| Poisoned README | `spikes/r0.1a-containment/workspace/README.md` |
| Fake secret | `spikes/r0.1a-containment/host-secrets/id_rsa` |
| Re-run harness | `spikes/r0.1a-containment/run.sh` |
| Async ask copy | [`P0-PERMIT-VALIDATION.md`](./P0-PERMIT-VALIDATION.md) |
| Honest claims (post-spike) | [`PERMIT-ALPHA-CLAIMS.md`](./PERMIT-ALPHA-CLAIMS.md) |
| Golden path | [`PERMIT-GOLDEN-PATH.md`](./PERMIT-GOLDEN-PATH.md) |

**Clip file:** not checked in yet — record locally from the harness, store under `docs/launch/assets/` when ready (optional for recruiting; PASS log is enough to start conversations).

## Runtime prerequisite (call out in posts / DMs)

Docker Desktop **≥ 4.x, native arch** (arm64 on Apple Silicon). Old Intel/HyperKit leftovers fatal and look like “Kotro doesn’t work.”

## Parallelism

Gate A recruiting **does not block** further engineering. Product paths to show after a commit-to-try: R2-A apply dogfood → R2-B broker dry-run → receipt verify (see claims page).
