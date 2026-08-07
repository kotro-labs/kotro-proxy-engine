# Kotro Permit — golden path (R4 demo narrative)

End-to-end story for demos. Prefer dogfood scripts over improvising live GitHub until host token + allow-once are intentional.

## Prerequisites

- Docker Desktop ≥ 4.x, **native arch**, daemon running
- Build: `cd rust && cargo build -p kotro-proxy`
- Optional live PR: `GITHUB_TOKEN` or `GH_TOKEN` on the **host only**

## Path A — Thesis (no GitHub)

Proves containment + Option A land without a PR.

```bash
./spikes/r0.1a-containment/run.sh   # optional ~60s containment evidence
./spikes/r2a-dogfood/run.sh         # dual-home + review/apply story
```

Talk track:

1. Agent runs under `run --permit` in Docker — host `~/.ssh` not mounted → ENOENT.
2. Edits land on a **staged** copy; you get `review.diff`.
3. `kotro-proxy apply --repo <live> --diff <review.diff>` after you look.

## Path B — Broker dry-run + receipt (no live PR)

```bash
./spikes/r2b-broker/run.sh
# Expect: DOGFOOD_OK + RECEIPT_OK
```

Talk track:

1. Broker refuses forged tokens / bad artifact hashes / merge scope.
2. Allow-once binds to artifact hash; second land → `token_consumed`.
3. Successful dry-run writes a **mediator-signed** land receipt.
4. `kotro-proxy receipt verify --trust …` → `chain_complete` only if mediator key is trusted.

## Path C — Live draft PR (operator-owned)

Only when you mean to push:

1. Complete a real `run --permit` with `land.mode=draft_pr`.
2. Capture `broker_session=`, `run_token=`, `review_diff=` from stdout.
3. Review the diff.
4. Dry-run first:

```bash
kotro-proxy broker draft-pr \
  --session "$SESSION" \
  --token "$TOKEN" \
  --allow-once-hash "sha256:$(shasum -a 256 "$DIFF" | awk '{print $1}')" \
  --dry-run
```

5. Live: set host `GITHUB_TOKEN`, drop `--dry-run`, use `--interactive` or matching `--allow-once-hash`.
6. Verify receipt; merge on GitHub yourself.

## Claims checklist before recording

Open [`PERMIT-ALPHA-CLAIMS.md`](./PERMIT-ALPHA-CLAIMS.md) and strike anything not on the “You may claim” table — especially “Kotro-only network” and “hypervisor.”
