#!/usr/bin/env bash
# R0.3 Permit acceptance harness — unit layer + staging safety.
# Spike evidence (#4–#7, #16/#25) is referenced, not re-run by default (use --spikes).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
RUST="$ROOT/rust"

RUN_SPIKES=0
for a in "$@"; do
  case "$a" in
    --spikes) RUN_SPIKES=1 ;;
  esac
done

echo "=== R0.3 permit-suite registry ==="
(cd "$RUST" && cargo test -q -p kotro-proxy permit::suite::tests::registry_covers_required_r03_cases -- --nocapture)

echo "=== Unit layer (permit::) ==="
(cd "$RUST" && cargo test -p kotro-proxy permit:: -- --test-threads=4)

echo "=== Staging safety (#28) ==="
bash spikes/r0.1b-topology/test-stage-safety.sh

if [[ "$RUN_SPIKES" -eq 1 ]]; then
  echo "=== Re-run containment spike (needs Docker) ==="
  bash spikes/r0.1a-containment/run.sh
  echo "=== Re-run topology spike (needs Docker) ==="
  bash spikes/r0.1b-topology/run.sh
else
  echo "=== Spike evidence (not re-run; pass --spikes to execute) ==="
  test -f spikes/r0.1a-containment/results/PASS-20260807.md
  test -f spikes/r0.1b-topology/results/PASS-20260807.md
  echo "PASS: evidence files present"
fi

echo "ALL R0.3 ACCEPTANCE LAYERS PASSED"
