#!/usr/bin/env bash
# R2-B thin broker dogfood — suite #21–#24 dry-run (no GITHUB_TOKEN / no live PR).
# Label: broker dry-run evidence — not a live draft-PR product claim.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RUST="$ROOT/rust"

echo "=== R2-B broker dogfood (#21–#24 dry-run) ==="

echo "--- broker unit gates ---"
(cd "$RUST" && cargo test -q -p kotro-proxy permit::broker:: -- --test-threads=4)

echo "--- CLI surface ---"
(cd "$RUST" && cargo build -q -p kotro-proxy)
BIN="$RUST/target/debug/kotro-proxy"
HELP=$("$BIN" broker 2>&1 || true)
echo "$HELP" | grep -q "draft-pr" || {
  echo "FAIL: broker CLI help missing draft-pr"
  echo "$HELP"
  exit 1
}
DRAFT=$("$BIN" broker draft-pr 2>&1 || true)
echo "$DRAFT" | grep -q "session" || {
  echo "FAIL: broker draft-pr usage missing --session"
  echo "$DRAFT"
  exit 1
}

echo "DOGFOOD_OK"
echo "--- receipt verify surface ---"
(cd "$RUST" && cargo test -q -p kotro-proxy permit::receipt:: -- --test-threads=4)
echo "RECEIPT_OK"
echo "next=kotro-proxy broker draft-pr --session <run.broker-session.json> --token <run_token> --allow-once-hash <sha256:…> --dry-run"
echo "next_receipt=kotro-proxy receipt verify --trust <store> <run.land-receipt.json>"
echo "live_pr=requires GITHUB_TOKEN on host + land.mode=draft_pr + interactive/allow-once (not claimed by this spike)"
