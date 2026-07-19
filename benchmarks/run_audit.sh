#!/usr/bin/env bash
# Cancel-storm load test with pre/post goroutine profiling via pprof.
# Target: baseline goroutine count == post-stress count (zero leak).
#
# Uses the frozen Go reference binary (bin/kotro-proxy-go), NOT the shipping
# Rust bin/kotro-proxy — Rust has no /debug/pprof/goroutine endpoint.
#
# Do NOT run this in parallel with run_rust_audit.sh — both bind :8080/:9000.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"

PROXY_URL="${KOTRO_PROXY_URL:-http://127.0.0.1:8080}"
# pprof is registered on the metrics/telemetry mux when metrics are enabled (default).
METRICS_URL="${KOTRO_METRICS_URL:-http://127.0.0.1:9090}"
PPROF_URL="${METRICS_URL}/debug/pprof/goroutine?debug=1"
GO_BIN="${GO_BIN:-$ROOT/bin/kotro-proxy-go}"
K6_VUS="${K6_VUS:-100}"
K6_DURATION="${K6_DURATION:-30s}"
COOLDOWN_SEC="${COOLDOWN_SEC:-3}"
GOROUTINE_TOLERANCE="${GOROUTINE_TOLERANCE:-5}"
CURL_TIMEOUT="${CURL_TIMEOUT:-5}"
AUDIT_LOG_DIR="${AUDIT_LOG_DIR:-${ROOT}/benchmarks/.audit-logs}"

if ! command -v k6 >/dev/null 2>&1; then
  echo "k6 not found. Install: brew install k6"
  exit 1
fi

mkdir -p "$AUDIT_LOG_DIR"
MOCK_LOG="${AUDIT_LOG_DIR}/go-mock.log"
PROXY_LOG="${AUDIT_LOG_DIR}/go-proxy.log"

curl_quiet() {
  curl -sf --connect-timeout 2 --max-time "$CURL_TIMEOUT" "$@"
}

goroutine_total() {
  curl_quiet "$PPROF_URL" | awk '/^goroutine profile: total/ { print $4; exit }'
}

show_failure_logs() {
  echo ""
  echo "=== Diagnostic tail (proxy) ==="
  tail -n 30 "$PROXY_LOG" 2>/dev/null || true
  echo ""
  echo "=== Diagnostic tail (k6) ==="
  tail -n 15 "${AUDIT_LOG_DIR}/k6-go.log" 2>/dev/null || true
}

wait_for_proxy() {
  for _ in $(seq 1 30); do
    if curl_quiet "${PROXY_URL}/healthz" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.2
  done
  echo "proxy not reachable at ${PROXY_URL} (see ${PROXY_LOG})"
  exit 1
}

START_STACK="${START_STACK:-1}"
if [[ "$START_STACK" == "1" ]]; then
  # Build Go reference + mock only — never `make build` (that installs Rust as bin/kotro-proxy).
  make mock go-proxy
  if [[ ! -x "$GO_BIN" ]]; then
    echo "Go proxy binary not found at ${GO_BIN}"
    exit 1
  fi

  # Avoid bare "kotro-proxy" — matches this repo path (kotro-proxy-engine).
  pkill -f 'bin/mock-upstream' 2>/dev/null || true
  pkill -f 'bin/kotro-proxy-go' 2>/dev/null || true
  pkill -f 'bin/kotro-proxy$' 2>/dev/null || true
  sleep 0.5
  # Dedicated bbolt path — do not reuse Rust redb kotro-cache.db.
  GO_CACHE_DB="${GO_CACHE_DB:-${ROOT}/kotro-cache-go.db}"
  rm -f "$GO_CACHE_DB"

  cleanup() {
    kill "$MOCK_PID" "$PROXY_PID" 2>/dev/null || true
  }
  trap cleanup EXIT

  : >"$MOCK_LOG"
  : >"$PROXY_LOG"

  MOCK_CHUNK_DELAY_MS="${MOCK_CHUNK_DELAY_MS:-80}" \
  MOCK_MIN_CHUNKS="${MOCK_MIN_CHUNKS:-48}" \
  bin/mock-upstream >>"$MOCK_LOG" 2>&1 &
  MOCK_PID=$!
  sleep 0.5

  KOTRO_UPSTREAM_URL=http://127.0.0.1:9000 \
  KOTRO_ENABLE_PPROF=true \
  KOTRO_CACHE_DB="$GO_CACHE_DB" \
  "$GO_BIN" >>"$PROXY_LOG" 2>&1 &
  PROXY_PID=$!
  sleep 0.5
fi

wait_for_proxy

if ! curl_quiet "$PPROF_URL" >/dev/null 2>&1; then
  echo "pprof not enabled. Start proxy with KOTRO_ENABLE_PPROF=true"
  exit 1
fi

echo "=== Step 1: Baseline goroutine allocation ==="
BASELINE="$(goroutine_total)"
echo "goroutine profile: total ${BASELINE}"

echo "=== Step 2: k6 cancel-storm (${K6_VUS} VUs, ${K6_DURATION}) ==="
AUDIT_VUS="$K6_VUS" AUDIT_DURATION="$K6_DURATION" KOTRO_PROXY_URL="$PROXY_URL" \
  k6 run --quiet --log-output=none benchmarks/cancel_storm.js \
  >"${AUDIT_LOG_DIR}/k6-go.log" 2>&1 || true

echo ""
echo "=== Step 3: Cooldown (${COOLDOWN_SEC}s) ==="
sleep "$COOLDOWN_SEC"

echo ""
echo "=== Step 4: Post-stress goroutine footprint ==="
POST="$(goroutine_total)"
echo "goroutine profile: total ${POST}"

DELTA=$((POST - BASELINE))
echo ""
echo "=== Audit summary ==="
echo "baseline:    ${BASELINE}"
echo "post-stress: ${POST}"
echo "delta:       ${DELTA} (tolerance ±${GOROUTINE_TOLERANCE})"
echo "proxy logs:  ${PROXY_LOG}"

if [[ "$DELTA" -le "$GOROUTINE_TOLERANCE" && "$DELTA" -ge -"$GOROUTINE_TOLERANCE" ]]; then
  echo "PASS: goroutine count returned to baseline (zero-leak within tolerance)"
  exit 0
fi

echo "FAIL: goroutine delta ${DELTA} exceeds tolerance ${GOROUTINE_TOLERANCE}"
show_failure_logs
exit 1
