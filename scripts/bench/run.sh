#!/usr/bin/env bash
# High-concurrency load test against the local proxy + mock upstream stack.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"

if ! command -v k6 >/dev/null 2>&1; then
  echo "k6 not found. Install: brew install k6"
  exit 1
fi

make build
pkill -f 'bin/mock-upstream|bin/kortolabs-proxy' 2>/dev/null || true
rm -f kortolabs-cache.db

cleanup() {
  kill "$MOCK_PID" "$PROXY_PID" 2>/dev/null || true
}
trap cleanup EXIT

bin/mock-upstream &
MOCK_PID=$!
sleep 0.5

KORTO_UPSTREAM_URL=http://127.0.0.1:9000 bin/kortolabs-proxy &
PROXY_PID=$!
sleep 0.5

# Warm cache for hit benchmark payloads.
curl -s -o /dev/null -N http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4","stream":true,"messages":[{"role":"system","content":"bench"},{"role":"user","content":"warm-openai"}]}'
curl -s -o /dev/null -N http://127.0.0.1:8080/v1/messages \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-3-5-sonnet-20241022","max_tokens":64,"stream":true,"system":"bench","messages":[{"role":"user","content":"warm-anthropic"}]}'

SCENARIO="${1:-all}"
case "$SCENARIO" in
  miss) k6 run scripts/bench/k6-cache-miss.js ;;
  hit)  k6 run scripts/bench/k6-cache-hit.js ;;
  anthropic) k6 run scripts/bench/k6-anthropic.js ;;
  mixed) k6 run scripts/bench/k6-mixed.js ;;
  all)
    echo "=== cache miss (OpenAI) ==="
    k6 run scripts/bench/k6-cache-miss.js
    echo "=== cache hit (OpenAI) ==="
    k6 run scripts/bench/k6-cache-hit.js
    echo "=== cache hit (Anthropic) ==="
    k6 run scripts/bench/k6-anthropic.js
    echo "=== mixed workload ==="
    k6 run scripts/bench/k6-mixed.js
    ;;
  *) echo "usage: $0 [miss|hit|anthropic|mixed|all]"; exit 1 ;;
esac
