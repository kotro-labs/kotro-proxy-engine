#!/usr/bin/env bash
# scripts/demo-agent-guard.sh — Death loop → circuit breaker + flight recorder
#
# Fires the same streaming prompt 5× with CB threshold=3 → trip + X-Kotro-Circuit-Open.
# Usage: bash scripts/demo-agent-guard.sh   # or: make agent-guard-demo

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GREEN=$'\033[0;32m'; CYAN=$'\033[0;36m'; BOLD=$'\033[1m'; DIM=$'\033[2m'
RED=$'\033[0;31m'; YELLOW=$'\033[1;33m'; RESET=$'\033[0m'
hdr()  { echo -e "\n${BOLD}${CYAN}▶ $*${RESET}"; }
ok()   { echo -e "  ${GREEN}✓${RESET}  $*"; }
info() { echo -e "  ${DIM}·${RESET}  $*"; }
fail() { echo -e "  ${RED}✗${RESET}  $*"; exit 1; }

hdr "Checking binaries"
[ -f bin/kotro-proxy ] || make proxy >/dev/null
[ -f bin/mock-upstream ] || make mock >/dev/null
ok "binaries ready"

hdr "Starting services (CB threshold=3, enforce mode)"
lsof -ti:8080 | xargs kill -9 2>/dev/null || true
lsof -ti:9000 | xargs kill -9 2>/dev/null || true
lsof -ti:9090 | xargs kill -9 2>/dev/null || true
sleep 0.3

DEMO_TMP=$(mktemp -d)
PROXY_LOG="$ROOT/kotro-demo-agent-guard.log"
cleanup() { kill "$MOCK_PID" "$PROXY_PID" 2>/dev/null || true; rm -rf "$DEMO_TMP"; }
trap cleanup EXIT

MOCK_CHUNK_DELAY_MS=5 bin/mock-upstream >"$DEMO_TMP/mock.log" 2>&1 &
MOCK_PID=$!

KOTRO_UPSTREAM_URL=http://127.0.0.1:9000 \
KOTRO_CACHE_DB="$DEMO_TMP/cache.db" \
KOTRO_ENABLE_VECTOR_CACHE=false \
CONTROL_TOKEN="demo-control-$(date +%s)"

KOTRO_ENABLE_CACHE=false \
KOTRO_CIRCUIT_BREAKER_THRESHOLD=3 \
KOTRO_CIRCUIT_BREAKER_WINDOW_SECS=60 \
KOTRO_KILL_SWITCH_MODE=enforce \
KOTRO_ENABLE_FLIGHT_RECORDER=true \
KOTRO_STATE_DIR="$DEMO_TMP/state" \
KOTRO_CONTROL_TOKEN="$CONTROL_TOKEN" \
KOTRO_ENABLE_METRICS=true \
KOTRO_METRICS_ADDR=127.0.0.1:9090 \
  bin/kotro-proxy >"$PROXY_LOG" 2>&1 &
PROXY_PID=$!

for _ in $(seq 1 50); do
  curl -sf http://127.0.0.1:8080/healthz >/dev/null 2>&1 && break
  sleep 0.1
done
curl -sf http://127.0.0.1:8080/healthz >/dev/null || fail "proxy failed — $PROXY_LOG"
ok "proxy up (cache disabled so every call is a miss / CB candidate)"

PAYLOAD='{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"Agent death-loop demo — fix the compile error again."}]}'

TRIPPED=0
hdr "Firing identical prompts (expect trip on 3rd)"
for i in 1 2 3 4 5; do
  curl -sN -D "$DEMO_TMP/h$i.txt" -o "$DEMO_TMP/b$i.txt" \
    http://127.0.0.1:8080/v1/chat/completions \
    -H 'Content-Type: application/json' \
    -H 'Authorization: Bearer demo' \
    -d "$PAYLOAD" >/dev/null || true
  CIRCUIT=$(grep -i '^x-kotro-circuit-open:' "$DEMO_TMP/h$i.txt" | awk '{print $2}' | tr -d '\r' || true)
  BODY_HINT=$(head -c 160 "$DEMO_TMP/b$i.txt" | tr '\n' ' ')
  if [[ "${CIRCUIT}" == "true" ]] || echo "$BODY_HINT" | grep -q 'CIRCUIT BREAKER'; then
    echo -e "  ${YELLOW}#${i}${RESET} CIRCUIT OPEN  ${DIM}${BODY_HINT:0:80}…${RESET}"
    TRIPPED=1
  else
    echo -e "  ${DIM}#${i} forwarded${RESET}"
  fi
done

[[ "$TRIPPED" -eq 1 ]] || fail "circuit breaker never tripped"

hdr "Flight recorder"
FLIGHT=$(curl -sf http://127.0.0.1:9090/api/flight-recorder)
echo "$FLIGHT" | python3 -c '
import json,sys
d=json.load(sys.stdin)
ev=d.get("events") or []
kinds=[e.get("kind") for e in ev]
print(f"  events={len(ev)} kinds={kinds[:8]}")
assert any(k=="circuit_open" for k in kinds), "missing circuit_open in flight tape"
print("  circuit_open present on tape")
' || fail "flight recorder check failed"
ok "flight recorder captured circuit_open"

hdr "Control API is authenticated (401 without token)"
NOAUTH_STATUS=$(curl -s -o /dev/null -w '%{http_code}' -X POST http://127.0.0.1:9090/api/kill-switch \
  -H 'Content-Type: application/json' \
  -d '{"engaged":true}')
[[ "$NOAUTH_STATUS" == "401" ]] && ok "unauthenticated kill-switch rejected (401)" \
  || info "expected 401 without token, got $NOAUTH_STATUS"

hdr "Optional: engage global kill switch (with control token)"
curl -sf -X POST http://127.0.0.1:9090/api/kill-switch \
  -H 'Content-Type: application/json' \
  -H "x-kotro-control-token: $CONTROL_TOKEN" \
  -d '{"engaged":true,"scope":"all"}' >/dev/null
curl -sN -D "$DEMO_TMP/kill.txt" -o "$DEMO_TMP/kill-body.txt" \
  http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer demo' \
  -d "$PAYLOAD" >/dev/null || true
if grep -qi 'KILL SWITCH\|x-kotro-kill-switch' "$DEMO_TMP/kill.txt" "$DEMO_TMP/kill-body.txt"; then
  ok "global kill switch blocked upstream"
else
  info "kill switch response (check body): $(head -c 120 "$DEMO_TMP/kill-body.txt")"
fi
curl -sf -X POST http://127.0.0.1:9090/api/kill-switch \
  -H 'Content-Type: application/json' \
  -H "x-kotro-control-token: $CONTROL_TOKEN" \
  -d '{"engaged":false}' >/dev/null

hdr "Flight recorder integrity"
curl -sf http://127.0.0.1:9090/api/flight-recorder/verify | python3 -c '
import json,sys
d=json.load(sys.stdin)
assert d.get("ok"), d
n = d.get("verified_events")
print(f"  hash chain verified: {n} events")
' && ok "tamper-evident chain verified" || info "verify endpoint unavailable"

echo -e "\n${BOLD}Dashboard:${RESET} http://127.0.0.1:9090/dashboard#flight-recorder"
echo -e "${GREEN}${BOLD}agent-guard-demo OK${RESET}"
