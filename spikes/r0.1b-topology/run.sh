#!/usr/bin/env bash
# R0.1b — Topology spike (Sol P0.2)
# Proves:
#   #16 agent reaches Kotro data-plane on shared net
#   #25 agent cannot reach host canary / gateway services
#   control-like service off agent net is unreachable
#   public egress still denied (recheck)
# Label: topology / staging contract spike — not a Permit demo
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
RESULTS="$ROOT/results"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$RESULTS/topology-$STAMP.txt"
IMG="${KOTRO_SPIKE_IMAGE:-python:3.12-slim}"
NET_AGENT="kotro-r01b-agent"
NET_UP="kotro-r01b-up"
CANARY_PORT="${KOTRO_CANARY_PORT:-18457}"
DP_NAME="kotro-r01b-dataplane"
UP_NAME="kotro-r01b-upstream"
CANARY_PID=""

mkdir -p "$RESULTS" "$ROOT/workspace"
printf 'ok\n' > "$ROOT/workspace/ok.txt"
exec > >(tee "$OUT") 2>&1

echo "=== R0.1b topology spike ==="
echo "label: Topology / host-canary spike"
echo "started: $STAMP"
echo

cleanup() {
  [[ -n "$CANARY_PID" ]] && kill "$CANARY_PID" 2>/dev/null || true
  docker rm -f "$DP_NAME" "$UP_NAME" >/dev/null 2>&1 || true
  docker network rm "$NET_AGENT" "$NET_UP" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker network rm "$NET_AGENT" "$NET_UP" >/dev/null 2>&1 || true
docker network create --internal "$NET_AGENT" >/dev/null
docker network create "$NET_UP" >/dev/null

# --- Host canary (NOT the data-plane): plain HTTP on host ---
python3 - <<PY &
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"CANARY_SECRET_SHOULD_NOT_LEAK")
    def log_message(self, *a):
        pass
HTTPServer(("0.0.0.0", int("$CANARY_PORT")), H).serve_forever()
PY
CANARY_PID=$!
sleep 0.5
# Prove canary works from host
curl -fsS "http://127.0.0.1:${CANARY_PORT}/" | grep -q CANARY_SECRET
echo "host canary listening on 0.0.0.0:${CANARY_PORT}"

# Gateway IP on agent network (Docker bridge gateway)
GW_IP=$(docker network inspect "$NET_AGENT" -f '{{(index .IPAM.Config 0).Gateway}}')
echo "agent network gateway: $GW_IP"

# --- Mock upstream (only on NET_UP) ---
docker rm -f "$UP_NAME" >/dev/null 2>&1 || true
docker run -d --name "$UP_NAME" --network "$NET_UP" "$IMG" \
  python - <<'PY' >/dev/null
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.end_headers()
        self.wfile.write(b"UPSTREAM_OK")
    def log_message(self, *a): pass
HTTPServer(("0.0.0.0", 9000), H).serve_forever()
PY
# docker run -d doesn't take heredoc as python stdin that way easily — use -c
docker rm -f "$UP_NAME" >/dev/null 2>&1 || true
docker run -d --name "$UP_NAME" --network "$NET_UP" "$IMG" \
  python -c 'from http.server import BaseHTTPRequestHandler,HTTPServer
class H(BaseHTTPRequestHandler):
  def do_GET(self):
    self.send_response(200); self.end_headers(); self.wfile.write(b"UPSTREAM_OK")
  def log_message(self,*a): pass
HTTPServer(("0.0.0.0",9000),H).serve_forever()' >/dev/null

# --- Mock Kotro data-plane: on AGENT net + UP net; holds "token" only here ---
docker rm -f "$DP_NAME" >/dev/null 2>&1 || true
docker run -d --name "$DP_NAME" --network "$NET_AGENT" \
  -e KOTRO_UPSTREAM=http://$UP_NAME:9000 \
  -e PROVIDER_TOKEN=super-secret-provider-token \
  "$IMG" \
  python -c 'import os,urllib.request
from http.server import BaseHTTPRequestHandler,HTTPServer
UP=os.environ["KOTRO_UPSTREAM"]; TOK=os.environ["PROVIDER_TOKEN"]
class H(BaseHTTPRequestHandler):
  def do_GET(self):
    if self.path.startswith("/v1/"):
      try:
        body=urllib.request.urlopen(UP+"/", timeout=3).read()
        self.send_response(200); self.end_headers()
        self.wfile.write(b"DATAPLANE_OK "+body)
      except Exception as e:
        self.send_response(502); self.end_headers(); self.wfile.write(str(e).encode())
    elif self.path.startswith("/control"):
      self.send_response(403); self.end_headers(); self.wfile.write(b"CONTROL_DENIED")
    else:
      self.send_response(404); self.end_headers()
  def log_message(self,*a): pass
HTTPServer(("0.0.0.0",8080),H).serve_forever()' >/dev/null
docker network connect "$NET_UP" "$DP_NAME"

DP_IP=$(docker inspect -f '{{range.NetworkSettings.Networks}}{{.IPAddress}} {{end}}' "$DP_NAME" | awk '{print $1}')
echo "data-plane IP on agent net: $DP_IP"
sleep 1

run_agent() {
  docker run --rm \
    --network "$NET_AGENT" \
    -e HOME=/home/agent \
    -e CANARY_PORT="$CANARY_PORT" \
    -e GW_IP="$GW_IP" \
    -e DP_IP="$DP_IP" \
    -v "$ROOT/workspace:/workspace:ro" \
    -w /workspace \
    "$IMG" \
    bash -lc "$1"
}

echo
echo "--- #16 agent → data-plane (+ mediated upstream) ---"
set +e
DP_OUT=$(run_agent 'python - <<'\''PY'\''
import os, urllib.request
ip = os.environ["DP_IP"]
try:
    body = urllib.request.urlopen(f"http://{ip}:8080/v1/chat", timeout=5).read().decode()
    print(body)
except Exception as e:
    print(type(e).__name__ + ":", e)
PY')
set -e
echo "$DP_OUT"
if echo "$DP_OUT" | grep -q 'DATAPLANE_OK' && echo "$DP_OUT" | grep -q 'UPSTREAM_OK'; then
  echo "RESULT #16: PASS — agent reached data-plane; upstream only via data-plane"
  FAIL16=0
elif echo "$DP_OUT" | grep -q 'DATAPLANE_OK'; then
  echo "RESULT #16: WARN — data-plane ok but upstream mediation incomplete"
  FAIL16=2
else
  echo "RESULT #16: FAIL — agent could not use data-plane"
  FAIL16=1
fi

echo
echo "--- #25 host canary / gateway must NOT be reachable from agent ---"
set +e
CAN_OUT=$(run_agent 'python - <<'\''PY'\''
import os, socket, urllib.request
port = int(os.environ["CANARY_PORT"])
gw = os.environ["GW_IP"]
targets = [
    ("host.docker.internal", port),
    (gw, port),
    ("172.17.0.1", port),
]
for host, p in targets:
    try:
        body = urllib.request.urlopen(f"http://{host}:{p}/", timeout=2).read().decode()
        print(f"CANARY_HIT {host}:{p} -> {body[:40]}")
    except Exception as e:
        print(f"CANARY_BLOCKED {host}:{p} -> {type(e).__name__}: {e}")
# Also raw TCP to gateway :22-ish / canary
for host in ("host.docker.internal", gw):
    s = socket.socket(); s.settimeout(2)
    try:
        s.connect((host, port))
        print(f"TCP_HIT {host}:{port}")
    except Exception as e:
        print(f"TCP_BLOCKED {host}:{port} -> {type(e).__name__}")
    finally:
        s.close()
PY')
set -e
echo "$CAN_OUT"
if echo "$CAN_OUT" | grep -q 'CANARY_SECRET\|CANARY_HIT\|TCP_HIT'; then
  echo "RESULT #25: FAIL — agent reached host canary/gateway service (internal:true ≠ host isolation)"
  echo "NOTE: product must add host firewall / bind data-plane-only before claiming sole window"
  FAIL25=1
else
  echo "RESULT #25: PASS — host canary/gateway probes blocked from agent net"
  FAIL25=0
fi

echo
echo "--- upstream direct (should fail; not on agent net) ---"
set +e
UP_OUT=$(run_agent 'python - <<'\''PY'\''
import socket, urllib.request
# Try common docker DNS name — should not resolve/route on internal-only agent net
try:
    print(urllib.request.urlopen("http://kotro-r01b-upstream:9000/", timeout=2).read())
except Exception as e:
    print(type(e).__name__ + ":", e)
PY')
set -e
echo "$UP_OUT"
if echo "$UP_OUT" | grep -q 'UPSTREAM_OK'; then
  echo "RESULT direct-upstream: FAIL"
  FAILUP=1
else
  echo "RESULT direct-upstream: PASS — cannot reach upstream except via data-plane"
  FAILUP=0
fi

echo
echo "--- provider token not in agent env ---"
set +e
TOK_OUT=$(run_agent 'python - <<'\''PY'\''
import os
for k,v in os.environ.items():
    if "TOKEN" in k.upper() or "SECRET" in k.upper() or "GITHUB" in k.upper():
        print(f"LEAK {k}={v}")
print("SCAN_DONE")
PY')
set -e
echo "$TOK_OUT"
if echo "$TOK_OUT" | grep -q '^LEAK'; then
  echo "RESULT token-scan: FAIL"
  FAILTOK=1
else
  echo "RESULT token-scan: PASS — no provider/github tokens in agent env"
  FAILTOK=0
fi

echo
echo "=== Summary ==="
echo "#16=$FAIL16 #25=$FAIL25 upstream=$FAILUP tokens=$FAILTOK"
echo "results_file=$OUT"
if [[ "$FAIL16" -eq 1 || "$FAIL25" -eq 1 || "$FAILUP" -eq 1 || "$FAILTOK" -eq 1 ]]; then
  echo "SPIKE: FAIL or host-reachability gap — see SOL P0.2 before claiming Kotro-only window"
  # #25 fail is informational hard-fail for "sole window" claim but we exit 1
  exit 1
fi
echo "SPIKE: PASS — dual-home shape holds on this Docker engine; host canary blocked"
exit 0
