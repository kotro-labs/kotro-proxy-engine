#!/usr/bin/env bash
# Kotro eval suite — runs compression, latency, parity, isolation, and cancel-storm checks.
# Writes benchmarks/eval-suite/.last-run.json and refreshes RESULTS.md.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"

SUITE_DIR="${ROOT}/benchmarks/eval-suite"
RUN_DIR="${SUITE_DIR}/.runs"
LAST_JSON="${SUITE_DIR}/.last-run.json"
PROXY_URL="${KORTO_PROXY_URL:-http://127.0.0.1:8080}"
UPDATE_RESULTS="${UPDATE_RESULTS:-1}"

mkdir -p "$RUN_DIR"

if ! command -v k6 >/dev/null 2>&1; then
  echo "k6 not found. Install: brew install k6"
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 not found"
  exit 1
fi

VERSION="$(git describe --tags --always 2>/dev/null || echo unknown)"
GIT_SHA="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
HOST="$(uname -srm 2>/dev/null || uname -a)"
DATE_UTC="$(date -u +%Y-%m-%d)"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"

K6_HIT_JSON="${RUN_DIR}/${RUN_ID}-k6-hit.json"
K6_MISS_JSON="${RUN_DIR}/${RUN_ID}-k6-miss.json"
PARITY_DIR="${RUN_DIR}/${RUN_ID}-parity"
mkdir -p "$PARITY_DIR"

echo "=== Kotro eval suite (${RUN_ID}) ==="

echo "→ compression workloads (offline)"
COMPRESSION_JSON="${RUN_DIR}/${RUN_ID}-compression.json"
go run "${SUITE_DIR}/measure.go" >"$COMPRESSION_JSON"

echo "→ building proxy stack"
make build >/dev/null
pkill -f 'bin/mock-upstream|bin/kortolabs-proxy' 2>/dev/null || true
rm -f kortolabs-cache.db

cleanup() {
  kill "$MOCK_PID" "$PROXY_PID" 2>/dev/null || true
}
trap cleanup EXIT

bin/mock-upstream >/dev/null 2>&1 &
MOCK_PID=$!
sleep 0.5

KORTO_UPSTREAM_URL=http://127.0.0.1:9000 \
KORTO_ENABLE_PPROF=true \
bin/kortolabs-proxy >/dev/null 2>&1 &
PROXY_PID=$!

for _ in $(seq 1 30); do
  if curl -sf --max-time 2 "${PROXY_URL}/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

echo "→ warming cache"
OPENAI_WARM='{"model":"gpt-4","stream":true,"messages":[{"role":"system","content":"bench"},{"role":"user","content":"warm-openai"}]}'
curl -s -o /dev/null -N "${PROXY_URL}/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "$OPENAI_WARM"

echo "→ W3 latency (k6 hit / miss)"
k6 run --quiet --summary-export="$K6_HIT_JSON" scripts/bench/k6-cache-hit.js
k6 run --quiet --summary-export="$K6_MISS_JSON" scripts/bench/k6-cache-miss.js

echo "→ W4 output parity (miss vs hit replay)"
PARITY_PAYLOAD='{"model":"gpt-4","stream":true,"messages":[{"role":"system","content":"bench"},{"role":"user","content":"warm-openai"}]}'
curl -s -N "${PROXY_URL}/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "$PARITY_PAYLOAD" >"${PARITY_DIR}/miss.sse" || true
curl -s -N "${PROXY_URL}/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "$PARITY_PAYLOAD" >"${PARITY_DIR}/hit.sse" || true

if cmp -s "${PARITY_DIR}/miss.sse" "${PARITY_DIR}/hit.sse"; then
  PARITY="pass"
else
  PARITY="fail"
fi

echo "→ W5 tenant isolation"
ISO_PAYLOAD='{"model":"gpt-4","stream":true,"messages":[{"role":"user","content":"isolation-probe"}]}'
cache_header() {
  local hdr
  hdr="$(mktemp)"
  curl -s -D "$hdr" -o /dev/null -N "${PROXY_URL}/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $1" \
    -d "$ISO_PAYLOAD"
  awk 'BEGIN{IGNORECASE=1} tolower($0) ~ /^x-korto.*cache:/ {gsub(/\r/,"",$2); print $2; exit}' "$hdr"
  rm -f "$hdr"
}
cache_header "tenant-alpha-token" >/dev/null
HIT_ALPHA="$(cache_header "tenant-alpha-token")"
HIT_BETA="$(cache_header "tenant-beta-token")"
ISOLATION_PASS="false"
if [[ "$HIT_ALPHA" == "HIT" && "$HIT_BETA" != "HIT" ]]; then
  ISOLATION_PASS="true"
fi

echo "→ W6 cancel storm (restart stack with audit-tuned mock)"
kill "$MOCK_PID" "$PROXY_PID" 2>/dev/null || true
sleep 0.5
rm -f kortolabs-cache.db

MOCK_CHUNK_DELAY_MS="${MOCK_CHUNK_DELAY_MS:-80}" \
MOCK_MIN_CHUNKS="${MOCK_MIN_CHUNKS:-48}" \
bin/mock-upstream >/dev/null 2>&1 &
MOCK_PID=$!
sleep 0.5

KORTO_UPSTREAM_URL=http://127.0.0.1:9000 \
KORTO_ENABLE_PPROF=true \
bin/kortolabs-proxy >>"${ROOT}/benchmarks/.audit-logs/go-proxy.log" 2>&1 &
PROXY_PID=$!
sleep 0.5

for _ in $(seq 1 30); do
  if curl -sf --max-time 2 "${PROXY_URL}/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

AUDIT_OUT="${RUN_DIR}/${RUN_ID}-audit.txt"
START_STACK=0 K6_VUS="${K6_VUS:-100}" K6_DURATION="${K6_DURATION:-30s}" \
  bash benchmarks/run_audit.sh >"$AUDIT_OUT" 2>&1
AUDIT_PASS=$?
BASELINE_G=$(awk '/^baseline:/ {print $2}' "$AUDIT_OUT")
POST_G=$(awk '/^post-stress:/ {print $2}' "$AUDIT_OUT")
DELTA_G=$(awk '/^delta:/ {print $2}' "$AUDIT_OUT")

MERGE_PAYLOAD="$(python3 - <<PY
import json
print(json.dumps({
    "version": "$VERSION",
    "git_sha": "$GIT_SHA",
    "date_utc": "$DATE_UTC",
    "host": """$HOST""",
    "run_id": "$RUN_ID",
    "compression": json.load(open("$COMPRESSION_JSON")),
    "k6_hit": json.load(open("$K6_HIT_JSON")),
    "k6_miss": json.load(open("$K6_MISS_JSON")),
    "parity": "$PARITY",
    "hit_alpha": "$HIT_ALPHA",
    "hit_beta": "$HIT_BETA",
    "isolation_pass": $( [[ "$ISOLATION_PASS" == "true" ]] && echo True || echo False ),
    "audit_pass": $( [[ $AUDIT_PASS -eq 0 ]] && echo True || echo False ),
    "baseline_g": "$BASELINE_G",
    "post_g": "$POST_G",
    "delta_g": "$DELTA_G",
}))
PY
)"

python3 "${SUITE_DIR}/results_tool.py" merge "$LAST_JSON" "$MERGE_PAYLOAD"

echo "Wrote ${LAST_JSON}"

if [[ "$UPDATE_RESULTS" == "1" ]]; then
  bash "${SUITE_DIR}/render-results.sh" "$LAST_JSON"
  echo "Updated ${SUITE_DIR}/RESULTS.md"
fi

if [[ "$AUDIT_PASS" -ne 0 ]]; then
  echo "WARN: cancel-storm audit failed (see ${AUDIT_OUT})"
fi

echo "PASS: eval suite complete"
