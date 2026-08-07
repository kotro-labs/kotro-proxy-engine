# R2-B thin broker dogfood

Proves suite **#21–#24** (forged token, artifact mismatch, allow-once deny, no merge) plus CLI surface for:

```text
kotro-proxy broker draft-pr --session … --token … [--interactive|--allow-once-hash …] [--dry-run]
kotro-proxy broker serve --session … [--bind 127.0.0.1:18999] [--dry-run]
```

## Run

```bash
./spikes/r2b-broker/run.sh
```

Expect `DOGFOOD_OK`. This spike does **not** open a live GitHub PR (no host `GITHUB_TOKEN` required).

## After a real `run --permit` with `land.mode=draft_pr`

1. Capture `broker_session=` and `run_token=` from the run output.
2. Review `review_diff=`.
3. Dry-run land:

```bash
kotro-proxy broker draft-pr \
  --session "$SESSION" \
  --token "$TOKEN" \
  --allow-once-hash "$(sha256sum "$DIFF" | awk '{print "sha256:"$1}')" \
  --dry-run
```

4. Live land (host-only token): set `GITHUB_TOKEN` / `GH_TOKEN`, drop `--dry-run`, use `--interactive` or a matching `--allow-once-hash`.
