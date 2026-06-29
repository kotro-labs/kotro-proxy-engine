#!/usr/bin/env bash
# Starts mock upstream (:9000) and proxy (:8080) for local offline testing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

make build

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

echo "Mock upstream: http://127.0.0.1:9000"
echo "Proxy:         http://127.0.0.1:8080/v1"
echo ""
echo "OpenAI streaming:"
echo 'curl -N http://127.0.0.1:8080/v1/chat/completions \'
echo '  -H "Content-Type: application/json" \'
echo '  -d '"'"'{"model":"gpt-4","stream":true,"messages":[{"role":"user","content":"hello kortolabs"}]}'"'"
echo ""
echo "Anthropic streaming:"
echo 'curl -N http://127.0.0.1:8080/v1/messages \'
echo '  -H "Content-Type: application/json" \'
echo '  -H "x-api-key: test-key" \'
echo '  -H "anthropic-version: 2023-06-01" \'
echo '  -d '"'"'{"model":"claude-3-5-sonnet-20241022","max_tokens":64,"stream":true,"messages":[{"role":"user","content":"hello anthropic"}]}'"'"
echo ""
wait
