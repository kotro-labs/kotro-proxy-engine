#!/usr/bin/env bash
# scripts/demo-cache-hit.sh — Hero demo: identical stream → MISS then HIT (<60s)
#
# No API keys. Uses mock upstream + exact prompt-state cache.
# Usage: bash scripts/demo-cache-hit.sh   # or: make demo-cache-hit

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GREEN=$'\033[0;32m'; CYAN=$'\033[0;36m'; BOLD=$'\033[1m'; DIM=$'\033[2m'
RED=$'\033[0;31m'; RESET=$'\033[0m'
hdr()  { echo -e "\n${BOLD}${CYAN}▶ $*${RESET}"; }
ok()   { echo -e "  ${GREEN}✓${RESET}  $*"; }
fail() { echo -e "  ${RED}✗${RESET}  $*"; exit 1; }

hdr "Checking binaries"
if [ ! -f bin/kotro-proxy ]; then
  make proxy >/dev/null
fi
if [ ! -f bin/mock-upstream ]; then
  make mock >/dev/null
fi
ok "binaries ready"

hdr "Starting mock + proxy (empty cache)"
lsof -ti:8080 | xargs kill -9 2>/dev/null || true
lsof -ti:9000 | xargs kill -9 2>/dev/null || true
sleep 0.3

DEMO_TMP=$(mktemp -d)
PROXY_LOG="$ROOT/kotro-demo-cache-hit.log"
cleanup() { kill "$MOCK_PID" "$PROXY_PID" 2>/dev/null || true; rm -rf "$DEMO_TMP"; }
trap cleanup EXIT

MOCK_CHUNK_DELAY_MS=5 bin/mock-upstream >"$DEMO_TMP/mock.log" 2>&1 &
MOCK_PID=$!

KOTRO_UPSTREAM_URL=http://127.0.0.1:9000 \
KOTRO_CACHE_DB="$DEMO_TMP/cache.db" \
KOTRO_STATE_DIR="$DEMO_TMP/state" \
KOTRO_ENABLE_VECTOR_CACHE=false \
KOTRO_ENABLE_METRICS=true \
KOTRO_METRICS_ADDR=127.0.0.1:9090 \
  bin/kotro-proxy >"$PROXY_LOG" 2>&1 &
PROXY_PID=$!

for _ in $(seq 1 50); do
  curl -sf http://127.0.0.1:8080/healthz >/dev/null 2>&1 && break
  sleep 0.1
done
curl -sf http://127.0.0.1:8080/healthz >/dev/null || fail "proxy did not start — see $PROXY_LOG"
ok "proxy :8080  metrics :9090"

PAYLOAD='{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"Kotro cache hero demo — identical prompt twice."}]}'

hdr "Request 1 (expect MISS)"
curl -sN -D "$DEMO_TMP/h1.txt" -o "$DEMO_TMP/b1.txt" \
  http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer demo' \
  -d "$PAYLOAD" >/dev/null
STATUS1=$(grep -i '^x-kotro-cache:' "$DEMO_TMP/h1.txt" | awk '{print $2}' | tr -d '\r' || true)
STATUS1=${STATUS1:-MISS}
echo "  cache header: ${STATUS1}"
[[ "${STATUS1}" == "HIT" ]] && fail "first request unexpectedly HIT"

hdr "Request 2 (expect HIT)"
curl -sN -D "$DEMO_TMP/h2.txt" -o "$DEMO_TMP/b2.txt" \
  http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer demo' \
  -d "$PAYLOAD" >/dev/null
STATUS2=$(grep -i '^x-kotro-cache:' "$DEMO_TMP/h2.txt" | awk '{print $2}' | tr -d '\r' || true)
echo "  cache header: ${STATUS2:-}"
[[ "${STATUS2}" == "HIT" ]] || fail "second request was not HIT (got '${STATUS2:-none}')"

ok "MISS → HIT on identical prompt-state"
echo -e "\n${BOLD}Dashboard:${RESET} http://127.0.0.1:9090/dashboard"
echo -e "${DIM}Flight recorder:${RESET} curl -s http://127.0.0.1:9090/api/flight-recorder | head"
echo -e "\n${GREEN}${BOLD}demo-cache-hit OK${RESET}"
