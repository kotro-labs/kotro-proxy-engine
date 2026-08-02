#!/usr/bin/env bash
# Run every Escape Lab env group against a local mock+proxy stack and merge
# results into one published matrix.
#
# Usage:
#   bash scripts/run-escape-lab-matrix.sh
#   OUT_DIR=/tmp/el bash scripts/run-escape-lab-matrix.sh
#
# Requires: rust/target/release/kotro-proxy (or builds it), go, python3.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${OUT_DIR:-/tmp/escape-lab-matrix}"
PROXY_BIN="${PROXY_BIN:-$ROOT/rust/target/release/kotro-proxy}"
MOCK_BIN="${MOCK_BIN:-$ROOT/bin/mock-upstream}"
TOKEN="${KOTRO_CONTROL_TOKEN:-escape-lab-local-token}"
MARKDOWN="${MARKDOWN:-$ROOT/docs/security/ESCAPE-LAB-MATRIX.md}"
MERGED_JSON="${MERGED_JSON:-$OUT_DIR/escape-lab-merged.json}"

mkdir -p "$OUT_DIR"

if [[ ! -x "$PROXY_BIN" ]]; then
  echo "building kotro-proxy (release)…"
  (cd "$ROOT/rust" && cargo build --release -p kotro-proxy)
fi
if [[ ! -x "$MOCK_BIN" ]]; then
  echo "building mock-upstream…"
  go build -o "$MOCK_BIN" ./cmd/mockupstream
fi

kill_port() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    local pids
    pids="$(lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)"
    if [[ -n "$pids" ]]; then
      # shellcheck disable=SC2086
      kill $pids 2>/dev/null || true
      sleep 0.3
    fi
  fi
}

start_mock() {
  kill_port 9000
  "$MOCK_BIN" >"$OUT_DIR/mock-upstream.log" 2>&1 &
  echo $! >"$OUT_DIR/mock-upstream.pid"
  for _ in $(seq 1 50); do
    curl -sf http://127.0.0.1:9000/healthz >/dev/null && return 0
    sleep 0.1
  done
  echo "mock upstream failed to start" >&2
  cat "$OUT_DIR/mock-upstream.log" >&2 || true
  return 1
}

start_proxy() {
  local env_flags="$1"
  kill_port 8080
  kill_port 9090
  rm -f /tmp/escape-lab-cache.db /tmp/escape-lab-cache.db-lock 2>/dev/null || true
  # shellcheck disable=SC2086
  env $env_flags \
    RUST_LOG=info \
    KOTRO_LISTEN_ADDR=127.0.0.1:8080 \
    KOTRO_UPSTREAM_URL=http://127.0.0.1:9000 \
    KOTRO_CACHE_DB=/tmp/escape-lab-cache.db \
    KOTRO_ENABLE_METRICS=true \
    KOTRO_METRICS_ADDR=127.0.0.1:9090 \
    KOTRO_ENABLE_INJECTION_SCAN=true \
    KOTRO_ENABLE_REDACTION=true \
    KOTRO_CONTROL_TOKEN="$TOKEN" \
    KOTRO_STATE_DIR="$OUT_DIR/state-$RANDOM" \
    "$PROXY_BIN" >"$OUT_DIR/kotro-proxy.log" 2>&1 &
  echo $! >"$OUT_DIR/kotro-proxy.pid"
  for _ in $(seq 1 80); do
    curl -sf http://127.0.0.1:8080/healthz >/dev/null && return 0
    sleep 0.1
  done
  echo "kotro-proxy failed to start" >&2
  cat "$OUT_DIR/kotro-proxy.log" >&2 || true
  return 1
}

stop_proxy() {
  if [[ -f "$OUT_DIR/kotro-proxy.pid" ]]; then
    kill "$(cat "$OUT_DIR/kotro-proxy.pid")" 2>/dev/null || true
    rm -f "$OUT_DIR/kotro-proxy.pid"
  fi
  kill_port 8080
  kill_port 9090
}

cleanup() {
  stop_proxy
  if [[ -f "$OUT_DIR/mock-upstream.pid" ]]; then
    kill "$(cat "$OUT_DIR/mock-upstream.pid")" 2>/dev/null || true
    rm -f "$OUT_DIR/mock-upstream.pid"
  fi
  kill_port 9000
}
trap cleanup EXIT

# group|env_flags  (must match .github/workflows/escape-lab.yml)
GROUPS=(
  "default|KOTRO_MODE=enforce"
  "injection-warn|KOTRO_MODE=enforce KOTRO_INJECTION_BLOCK=false"
  "injection-block|KOTRO_MODE=enforce KOTRO_INJECTION_BLOCK=true"
  "budget|KOTRO_MODE=enforce KOTRO_SESSION_TOKEN_BUDGET=200 KOTRO_BUDGET_BLOCK=true"
  "mode-audit|KOTRO_MODE=audit KOTRO_INJECTION_BLOCK=true"
  "mode-disabled|KOTRO_MODE=disabled KOTRO_INJECTION_BLOCK=true"
)

start_mock

JSON_FILES=()
for entry in "${GROUPS[@]}"; do
  group="${entry%%|*}"
  flags="${entry#*|}"
  echo ""
  echo "==> env group: $group  ($flags)"
  stop_proxy
  start_proxy "$flags"
  out="$OUT_DIR/escape-lab-${group}.json"
  python3 "$ROOT/scripts/escape-lab.py" \
    --target http://127.0.0.1:8080 \
    --control-target http://127.0.0.1:9090 \
    --control-token "$TOKEN" \
    --env-group "$group" \
    --out "$out" \
    --markdown "$OUT_DIR/ESCAPE-LAB-${group}.md"
  JSON_FILES+=("$out")
done

echo ""
echo "==> merging ${#JSON_FILES[@]} group result files"
python3 "$ROOT/scripts/escape-lab.py" \
  --merge "${JSON_FILES[@]}" \
  --out "$MERGED_JSON" \
  --markdown "$MARKDOWN"

echo ""
echo "wrote $MARKDOWN"
echo "wrote $MERGED_JSON"
