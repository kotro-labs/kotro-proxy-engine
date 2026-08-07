#!/usr/bin/env bash
# R2-A / R2.3b dogfood — dual-home dataplane smoke + review/apply land.
# Label: dogfood / Gate B partial — not a broker demo.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RUST="$ROOT/rust"
TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

echo "=== R2-A / R2.3b dogfood ==="

echo "--- unit gates ---"
(cd "$RUST" && cargo test -q -p kotro-proxy permit:: -- --test-threads=4)

if ! docker info >/dev/null 2>&1; then
  echo "SKIP docker dual-home (docker unavailable)"
  echo "DOGFOOD_PARTIAL_OK"
  exit 0
fi

IMG="${KOTRO_DATAPLANE_IMAGE:-python:3.12-slim}"
docker pull -q "$IMG" >/dev/null || true
AGENT_NET="kotro-dogfood-agent-$$"
UP_NET="kotro-dogfood-up-$$"
DP="kotro-dogfood-dp-$$"
cleanup_docker() {
  docker rm -f "$DP" >/dev/null 2>&1 || true
  docker network rm "$AGENT_NET" "$UP_NET" >/dev/null 2>&1 || true
  rm -rf "$TMP"
}
trap cleanup_docker EXIT

echo "--- dual-home: agent reaches dataplane; provider token stays on dataplane ---"
docker network create --internal "$AGENT_NET" >/dev/null
docker network create "$UP_NET" >/dev/null
docker run -d --name "$DP" --network "$AGENT_NET" \
  -e PROVIDER_TOKEN=super-secret-must-not-leak \
  "$IMG" \
  python -c 'from http.server import BaseHTTPRequestHandler,HTTPServer
class H(BaseHTTPRequestHandler):
  def do_GET(self):
    if self.path.startswith("/control"):
      self.send_response(403); self.end_headers(); self.wfile.write(b"CONTROL_DENIED"); return
    self.send_response(200); self.end_headers(); self.wfile.write(b"DATAPLANE_OK")
  def log_message(self,*a): pass
HTTPServer(("0.0.0.0",8080),H).serve_forever()' >/dev/null
docker network connect "$UP_NET" "$DP"
DP_IP=$(docker inspect -f "{{(index .NetworkSettings.Networks \"$AGENT_NET\").IPAddress}}" "$DP")
echo "dataplane_ip=$DP_IP"

REPO="$TMP/live"
mkdir -p "$REPO"
git -C "$REPO" init -q
git -C "$REPO" config user.email "dogfood@kotro.dev"
git -C "$REPO" config user.name "dogfood"
echo "version=1" > "$REPO/app.txt"
git -C "$REPO" add app.txt
git -C "$REPO" commit -q -m init

STAGE="$TMP/stage"
mkdir -p "$STAGE"
git -C "$REPO" archive HEAD | tar -x -C "$STAGE"

set +e
OUT=$(docker run --rm --network "$AGENT_NET" \
  -e HOME=/home/agent \
  -e KOTRO_RUN_TOKEN=dogfood-run-token \
  -e KOTRO_DATAPLANE_URL="http://${DP_IP}:8080" \
  -e KOTRO_BROKER_URL="http://${DP_IP}:8080" \
  -v "$STAGE:/workspace:rw" -w /workspace \
  "$IMG" \
  python -c '
import os, urllib.request
url=os.environ["KOTRO_DATAPLANE_URL"]
body=urllib.request.urlopen(url+"/", timeout=5).read()
print("DP:", body.decode())
assert b"DATAPLANE_OK" in body
assert "GITHUB_TOKEN" not in os.environ
assert "OPENAI_API_KEY" not in os.environ
assert "ANTHROPIC_API_KEY" not in os.environ
assert "PROVIDER_TOKEN" not in os.environ
assert os.environ.get("KOTRO_RUN_TOKEN")
open("app.txt","w").write("version=2\n")
print("EDIT_OK")
')
RC=$?
set -e
echo "$OUT"
test "$RC" -eq 0
echo "$OUT" | grep -q DATAPLANE_OK
echo "$OUT" | grep -q EDIT_OK
echo "$OUT" | grep -vq super-secret

echo "--- review diff → apply ---"
DIFF="$TMP/review.diff"
cat > "$DIFF" <<'EOF'
--- a/app.txt
+++ b/app.txt
@@ -1 +1 @@
-version=1
+version=2
EOF
(cd "$RUST" && cargo run -q -p kotro-proxy -- apply --repo "$REPO" --diff "$DIFF" --check)
(cd "$RUST" && cargo run -q -p kotro-proxy -- apply --repo "$REPO" --diff "$DIFF")
grep -q 'version=2' "$REPO/app.txt"
echo "APPLY_OK"

cleanup_docker
trap cleanup EXIT
echo "DOGFOOD_OK"
