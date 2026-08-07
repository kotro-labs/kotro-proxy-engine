#!/usr/bin/env bash
# R0.1a — Containment feasibility spike (NOT a Permit / receipt demo)
# Proves: #4 shell secret read, #5 Python secret read, #6 domain egress, #7 IP egress
# Records failure mode: prefer FileNotFoundError/ENOENT over PermissionError/EACCES
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
RESULTS="$ROOT/results"
NET="kotro-r01a-agent"
IMG="${KOTRO_SPIKE_IMAGE:-python:3.12-slim}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$RESULTS/spike-$STAMP.txt"

mkdir -p "$RESULTS"
exec > >(tee "$OUT") 2>&1

echo "=== Containment feasibility spike ==="
echo "label: Containment feasibility spike"
echo "started: $STAMP"
echo "image: $IMG"
echo

# Host secret lives OUTSIDE any bind mount into the agent.
HOST_SECRET_ABS="$ROOT/host-secrets/id_rsa"
WORKSPACE_ABS="$ROOT/workspace"
test -f "$HOST_SECRET_ABS"
test -d "$WORKSPACE_ABS"

# Fresh internal network = Docker deny-all egress (real enforcement).
docker network rm "$NET" >/dev/null 2>&1 || true
docker network create --internal "$NET" >/dev/null

cleanup() {
  docker network rm "$NET" >/dev/null 2>&1 || true
}
trap cleanup EXIT

run_agent() {
  # Mount only workspace at /workspace.
  # HOME=/home/agent with NO ~/.ssh mount — host secret must be unreachable.
  # Do NOT mount host-secrets.
  docker run --rm \
    --network "$NET" \
    -e HOME=/home/agent \
    -v "$WORKSPACE_ABS:/workspace:ro" \
    -w /workspace \
    "$IMG" \
    bash -lc "$1"
}

echo "--- #4 shell read of ~/.ssh/id_rsa ---"
set +e
SHELL_OUT=$(run_agent 'mkdir -p "$HOME/.ssh"; cat "$HOME/.ssh/id_rsa" 2>&1; echo EXIT:$?' )
SHELL_RC=$?
set -e
echo "$SHELL_OUT"
if echo "$SHELL_OUT" | grep -q 'FAKE-SSH-PRIVATE-KEY'; then
  echo "RESULT #4: FAIL — secret content leaked"
  FAIL4=1
elif echo "$SHELL_OUT" | grep -qiE 'No such file|cannot open|No such'; then
  echo "RESULT #4: PASS — denied via ENOENT / not present (wanted)"
  FAIL4=0
elif echo "$SHELL_OUT" | grep -qiE 'Permission denied|Operation not permitted'; then
  echo "RESULT #4: WARN — Permission denied (path may be mounted/blocked; investigate)"
  FAIL4=2
else
  echo "RESULT #4: FAIL — unexpected outcome"
  FAIL4=1
fi
echo

echo "--- #5 Python read of ~/.ssh/id_rsa ---"
set +e
PY_OUT=$(run_agent 'python - <<'\''PY'\''
import os, traceback
path = os.path.expanduser("~/.ssh/id_rsa")
try:
    data = open(path).read()
    print("LEAKED:", repr(data[:80]))
except Exception as e:
    print(type(e).__name__ + ":", e)
    raise SystemExit(1)
PY' )
PY_RC=$?
set -e
echo "$PY_OUT"
if echo "$PY_OUT" | grep -q 'LEAKED:'; then
  echo "RESULT #5: FAIL — secret content leaked"
  FAIL5=1
elif echo "$PY_OUT" | grep -q 'FileNotFoundError'; then
  echo "RESULT #5: PASS — FileNotFoundError (not mounted; wanted)"
  FAIL5=0
elif echo "$PY_OUT" | grep -q 'PermissionError'; then
  echo "RESULT #5: WARN — PermissionError (mounted but blocked; bad property)"
  FAIL5=2
else
  echo "RESULT #5: FAIL — unexpected outcome"
  FAIL5=1
fi
echo

echo "--- #6 hostile-domain egress (DNS + HTTP) ---"
# Sol P0: Docker --internal can still resolve external DNS via embedded resolver.
# PASS only if BOTH external DNS and external HTTP fail as exfil channels.
# Unique label logged for Gate A correlation when an external observer exists.
DNS_LABEL="kotro-r01a-$(date +%s)-$RANDOM.example.com"
set +e
DOM_OUT=$(docker run --rm \
  --network "$NET" \
  -e HOME=/home/agent \
  -e DNS_LABEL="$DNS_LABEL" \
  -v "$WORKSPACE_ABS:/workspace:ro" \
  -w /workspace \
  "$IMG" \
  bash -lc 'python - <<'\''PY'\''
import socket, urllib.request, os
label = os.environ["DNS_LABEL"]
try:
    answers = socket.getaddrinfo(label, 80)
    print("DNS_OK:", answers[:1])
except Exception as e:
    print(type(e).__name__ + ":", e)
try:
    answers = socket.getaddrinfo("example.com", 80)
    print("DNS_EXT_OK:", answers[:1])
except Exception as e:
    print("DNS_EXT_" + type(e).__name__ + ":", e)
try:
    urllib.request.urlopen("http://example.com", timeout=3)
    print("HTTP_OK")
except Exception as e:
    print(type(e).__name__ + ":", e)
PY' )
set -e
echo "dns_label=$DNS_LABEL"
echo "$DOM_OUT"
FAIL6=0
if echo "$DOM_OUT" | grep -q 'DNS_OK:'; then
  echo "RESULT #6: FAIL — unique-label DNS returned answers (exfil/resolver channel)"
  FAIL6=1
elif echo "$DOM_OUT" | grep -q 'DNS_EXT_OK:'; then
  echo "RESULT #6: FAIL — external DNS (example.com) returned answers; internal net does not block resolver forwarding"
  FAIL6=1
elif echo "$DOM_OUT" | grep -q 'HTTP_OK'; then
  echo "RESULT #6: FAIL — external HTTP succeeded"
  FAIL6=1
else
  echo "RESULT #6: PASS — external DNS and HTTP both failed as channels"
  FAIL6=0
fi
echo

echo "--- #7 hostile-IP egress ---"
set +e
IP_OUT=$(run_agent 'python - <<'\''PY'\''
import socket
s = socket.socket()
s.settimeout(3)
try:
    # 1.1.1.1:443 — public IP, should fail on internal network
    s.connect(("1.1.1.1", 443))
    print("CONNECT_OK")
except Exception as e:
    print(type(e).__name__ + ":", e)
finally:
    s.close()
PY' )
set -e
echo "$IP_OUT"
if echo "$IP_OUT" | grep -q 'CONNECT_OK'; then
  echo "RESULT #7: FAIL — IP connect succeeded"
  FAIL7=1
else
  echo "RESULT #7: PASS — IP connect blocked (see exception class above)"
  FAIL7=0
fi
echo

echo "=== Summary ==="
echo "#4=$FAIL4 #5=$FAIL5 #6=$FAIL6 #7=$FAIL7"
echo "0=pass 1=fail 2=warn(PermissionError/over-mount)"
echo "results_file=$OUT"

if [[ "$FAIL4" -eq 1 || "$FAIL5" -eq 1 || "$FAIL6" -eq 1 || "$FAIL7" -eq 1 ]]; then
  echo "SPIKE: HARD FAIL — rethink thesis before more plan/docs"
  exit 1
fi
if [[ "$FAIL4" -eq 2 || "$FAIL5" -eq 2 ]]; then
  echo "SPIKE: PASS WITH WARN — containment holds but failure mode is PermissionError; investigate mounts"
  exit 0
fi
echo "SPIKE: PASS — containment feasible under Docker internal network + selective mounts"
exit 0
