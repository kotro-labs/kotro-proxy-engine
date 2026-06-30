#!/usr/bin/env bash
# Cancel-storm load test with pre/post RSS + OS thread profiling for the Rust proxy.
# Target: thread delta == 0 and RSS delta within tolerance (RAII reclamation).
#
# Do NOT run this in parallel with run_audit.sh — both bind :8080/:9000.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"

PROXY_URL="${KORTO_PROXY_URL:-http://127.0.0.1:8080}"
K6_VUS="${K6_VUS:-100}"
K6_DURATION="${K6_DURATION:-30s}"
COOLDOWN_SEC="${COOLDOWN_SEC:-3}"
# 100 VUs: connection pool + allocator plateau typically 50–65 MB above idle baseline.
MEM_TOLERANCE_KB="${MEM_TOLERANCE_KB:-65536}"
CURL_TIMEOUT="${CURL_TIMEOUT:-5}"
RUST_BIN="${RUST_BIN:-$ROOT/rust/target/release/korto-proxy}"
SKIP_RUST_BUILD="${SKIP_RUST_BUILD:-0}"
AUDIT_LOG_DIR="${AUDIT_LOG_DIR:-${ROOT}/benchmarks/.audit-logs}"

if ! command -v k6 >/dev/null 2>&1; then
  echo "k6 not found. Install: brew install k6"
  exit 1
fi

mkdir -p "$AUDIT_LOG_DIR"
MOCK_LOG="${AUDIT_LOG_DIR}/rust-mock.log"
PROXY_LOG="${AUDIT_LOG_DIR}/rust-proxy.log"

curl_quiet() {
  curl -sf --connect-timeout 2 --max-time "$CURL_TIMEOUT" "$@"
}

thread_count() {
  local pid="$1"
  # macOS: ps -M lists one row per thread; skip the header row.
  ps -M -p "$pid" 2>/dev/null | tail -n +2 | wc -l | tr -d ' '
}

rss_kb() {
  local pid="$1"
  ps -o rss= -p "$pid" 2>/dev/null | tr -d ' '
}

show_failure_logs() {
  echo ""
  echo "=== Diagnostic tail (proxy) ==="
  tail -n 30 "$PROXY_LOG" 2>/dev/null || true
  echo ""
  echo "=== Diagnostic tail (k6) ==="
  tail -n 15 "${AUDIT_LOG_DIR}/k6-rust.log" 2>/dev/null || true
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

find_proxy_pid() {
  if [[ -n "${PROXY_PID:-}" ]] && kill -0 "$PROXY_PID" 2>/dev/null; then
    echo "$PROXY_PID"
    return
  fi
  # Avoid matching Cursor extension hosts (workspace path contains korto-proxy-engine).
  pgrep -f "${RUST_BIN}" 2>/dev/null | head -n 1 || true
}

START_STACK="${START_STACK:-1}"
if [[ "$START_STACK" == "1" ]]; then
  make build
  if [[ "$SKIP_RUST_BUILD" != "1" || ! -x "$RUST_BIN" ]]; then
    echo "Building Rust release binary (set SKIP_RUST_BUILD=1 to skip if already built)..."
    make rust-build
  fi

  if [[ ! -x "$RUST_BIN" ]]; then
    echo "Rust binary not found at ${RUST_BIN}"
    exit 1
  fi

  # Never pkill bare "korto-proxy" — that matches this repo path (korto-proxy-engine)
  # and can terminate Cursor's extension host.
  pkill -f 'bin/mock-upstream' 2>/dev/null || true
  pkill -f "${RUST_BIN}" 2>/dev/null || true
  sleep 0.5
  rm -f kortolabs-cache.db

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

  RUST_LOG=warn \
  KORTO_LISTEN_ADDR=":8080" \
  KORTO_UPSTREAM_URL="http://127.0.0.1:9000" \
  KORTO_CACHE_DB="${ROOT}/kortolabs-cache.db" \
  "$RUST_BIN" >>"$PROXY_LOG" 2>&1 &
  PROXY_PID=$!
  sleep 0.5
fi

wait_for_proxy

PID="$(find_proxy_pid)"
if [[ -z "$PID" ]]; then
  echo "Error: Rust proxy engine binary is not running on the host system."
  exit 1
fi

echo "=== Step 1: Querying Baseline Rust Footprint (pid=${PID}) ==="
THREADS_BASELINE="$(thread_count "$PID")"
MEM_BASELINE="$(rss_kb "$PID")"
echo "Baseline OS Threads: ${THREADS_BASELINE}"
echo "Baseline Memory (RSS): ${MEM_BASELINE} KB"

echo "=== Step 2: k6 cancel-storm (${K6_VUS} VUs, ${K6_DURATION}) ==="
AUDIT_VUS="$K6_VUS" AUDIT_DURATION="$K6_DURATION" KORTO_PROXY_URL="$PROXY_URL" \
  k6 run --quiet --log-output=none benchmarks/cancel_storm.js \
  >"${AUDIT_LOG_DIR}/k6-rust.log" 2>&1 || true

echo ""
echo "=== Step 3: Cooldown (${COOLDOWN_SEC}s) ==="
sleep "$COOLDOWN_SEC"

# Re-resolve PID in case the process restarted (it should not).
PID="$(find_proxy_pid)"
if [[ -z "$PID" ]]; then
  echo "FAIL: Rust proxy exited during cancel-storm"
  show_failure_logs
  exit 1
fi

echo ""
echo "=== Step 4: Extracting Post-Stress Rust Footprint (pid=${PID}) ==="
THREADS_POST="$(thread_count "$PID")"
MEM_POST="$(rss_kb "$PID")"
echo "Post-Stress OS Threads: ${THREADS_POST}"
echo "Post-Stress Memory (RSS): ${MEM_POST} KB"

THREAD_DELTA=$((THREADS_POST - THREADS_BASELINE))
MEM_DELTA=$((MEM_POST - MEM_BASELINE))

echo ""
echo "=== Resource Reclamation Evaluation ==="
echo "Thread Delta: ${THREAD_DELTA} (Target: 0)"
echo "Memory Creep: ${MEM_DELTA} KB (Max Allowed: ${MEM_TOLERANCE_KB} KB)"
echo "proxy logs:  ${PROXY_LOG}"

if [[ "$THREAD_DELTA" -eq 0 && "$MEM_DELTA" -le "$MEM_TOLERANCE_KB" ]]; then
  echo "PASS: Concurrency contract validated. Threads are completely invariant, and memory bounds stabilized within acceptable pool limits."
  exit 0
fi

echo "FAIL: Resource leakage or unexpected scaling behavior detected."
show_failure_logs
exit 1
