#!/usr/bin/env bash
# Vegeta throughput benchmark (alternative to k6). Requires: go install github.com/tsenart/vegeta@latest
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

export PATH="$(go env GOPATH 2>/dev/null)/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if ! command -v vegeta >/dev/null 2>&1; then
  echo "vegeta not found. Install: go install github.com/tsenart/vegeta@latest"
  exit 1
fi

make build
pkill -f 'bin/mock-upstream|bin/kortolabs-proxy' 2>/dev/null || true
rm -f kortolabs-cache.db

cleanup() { kill "$MOCK_PID" "$PROXY_PID" 2>/dev/null || true; }
trap cleanup EXIT

bin/mock-upstream & MOCK_PID=$!; sleep 0.4
KORTO_UPSTREAM_URL=http://127.0.0.1:9000 bin/kortolabs-proxy & PROXY_PID=$!; sleep 0.4

curl -s -o /dev/null -N http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4","stream":true,"messages":[{"role":"user","content":"warm-openai"}]}'

echo 'POST http://127.0.0.1:8080/v1/chat/completions
Content-Type: application/json
@scripts/bench/payloads/openai-hit.json' | vegeta attack -duration=15s -rate=100 | vegeta report
