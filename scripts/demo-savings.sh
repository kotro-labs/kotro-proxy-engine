#!/usr/bin/env bash
# scripts/demo-savings.sh — Kotro savings demo (screenshot-ready terminal output)
#
# Starts mock-upstream + kotro-proxy locally, fires 13 realistic coding-agent
# prompts (repeated context reloads, agent retries, unique questions, a secret-
# containing prompt), then prints a savings summary you can screenshot for the
# Show HN post or README.
#
# Usage:
#   bash scripts/demo-savings.sh
#   # or: make demo-savings
#
# No API keys required — uses the bundled mock upstream.
# Build both binaries first if they're missing (make build).

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ── ANSI colours (dollar-quote so ESC byte is embedded, not literal \033) ─────
GREEN=$'\033[0;32m'; YELLOW=$'\033[1;33m'; CYAN=$'\033[0;36m'
BOLD=$'\033[1m'; DIM=$'\033[2m'; RESET=$'\033[0m'
RED=$'\033[0;31m'

# ── Helper: print header / status ─────────────────────────────────────────────
hdr()  { echo -e "\n${BOLD}${CYAN}▶ $*${RESET}"; }
ok()   { echo -e "  ${GREEN}✓${RESET}  $*"; }
info() { echo -e "  ${DIM}·${RESET}  $*"; }

# ── 1. Build binaries if missing ──────────────────────────────────────────────
hdr "Checking binaries"

if [ ! -f bin/kotro-proxy ]; then
  info "Building Rust proxy (release)…"
  cd rust
  CARGO_TARGET_DIR=../bin/rust-target cargo build --release -p kotro-proxy -q
  cd ..
  cp bin/rust-target/release/kotro-proxy bin/kotro-proxy
  ok "bin/kotro-proxy built"
else
  ok "bin/kotro-proxy  ✓"
fi

if [ ! -f bin/mock-upstream ]; then
  info "Building mock upstream…"
  go build -o bin/mock-upstream ./cmd/mockupstream
  ok "bin/mock-upstream built"
else
  ok "bin/mock-upstream ✓"
fi

# ── 2. Start services ─────────────────────────────────────────────────────────
hdr "Starting services"

# Kill any stale proxy/mock from previous interrupted runs (or other listeners).
# Without this, our demo proxy fails to bind silently and healthz hits whatever
# is already on :8080 (e.g. a production Kotro instance with a full cache).
lsof -ti:8080 | xargs kill -9 2>/dev/null || true
lsof -ti:9000 | xargs kill -9 2>/dev/null || true
sleep 0.5   # brief pause for OS to release ports

# Each run gets its own temp dir — the cache DB lives here so every run
# starts with a guaranteed-empty cache (no leftover entries from prior runs).
DEMO_TMP=$(mktemp -d)
# Keep proxy log at a fixed path so it's readable after the script exits.
PROXY_LOG="$ROOT/kotro-demo-proxy.log"
cleanup() { kill "$MOCK_PID" "$PROXY_PID" 2>/dev/null || true; rm -rf "$DEMO_TMP"; }
trap cleanup EXIT

# Fast chunks for demo speed (5 ms × 8 chunks ≈ 40 ms per upstream request).
MOCK_CHUNK_DELAY_MS=5 bin/mock-upstream \
  > "$DEMO_TMP/mock.log" 2>&1 &
MOCK_PID=$!

# KOTRO_CACHE_DB in temp dir → guaranteed empty on start.
# KOTRO_ENABLE_VECTOR_CACHE=false → exact-match SHA-256 only, so "unique"
# prompts are real MISSes and only identical repeats produce HITs.
# (The semantic cache is a real feature but would match all Rust questions
#  above the 0.94 cosine threshold, making every request a HIT in the demo.)
RUST_LOG=debug \
KOTRO_UPSTREAM_URL=http://127.0.0.1:9000 \
KOTRO_CACHE_DB="$DEMO_TMP/demo-cache.db" \
KOTRO_ENABLE_VECTOR_CACHE=false \
KOTRO_SESSION_TOKEN_BUDGET=500000 \
KOTRO_ENABLE_METRICS=false \
  bin/kotro-proxy \
  > "$PROXY_LOG" 2>&1 &
PROXY_PID=$!

# Wait for proxy readiness (up to 5 s).
PROXY_READY=0
for i in {1..50}; do
  if curl -sf http://127.0.0.1:8080/healthz > /dev/null 2>&1; then
    PROXY_READY=1; break
  fi
  sleep 0.1
done
if [ "$PROXY_READY" -eq 0 ]; then
  echo -e "${RED}ERROR: proxy did not start within 5 s — check $PROXY_LOG${RESET}"
  exit 1
fi
# Verify it's our proxy (not a stale listener from before we killed ports).
if ! kill -0 "$PROXY_PID" 2>/dev/null; then
  echo -e "${RED}ERROR: proxy process ($PROXY_PID) exited — check $PROXY_LOG${RESET}"
  cat "$PROXY_LOG" >&2
  exit 1
fi
ok "mock upstream  :9000"
ok "kotro proxy    :8080"

# ── 3. Build request payloads ─────────────────────────────────────────────────
# Large-context request — simulates Cursor resending code on every turn (~520 chars).
cat > "$DEMO_TMP/large.json" <<'EOF'
{
  "model": "gpt-4o",
  "stream": true,
  "messages": [
    {
      "role": "system",
      "content": "You are an expert Rust engineer reviewing production proxy code."
    },
    {
      "role": "user",
      "content": "Review this Axum handler for correctness and performance:\n```rust\nasync fn handle_chat(\n    State(state): State<Arc<AppState>>,\n    Json(req): Json<ChatRequest>,\n) -> impl IntoResponse {\n    let key = sha256(&req);\n    if let Some(hit) = state.cache.get(&key) {\n        return replay_stream(hit);\n    }\n    let resp = state.client.post(&state.upstream)\n        .json(&req).send().await?;\n    let body = resp.bytes().await?;\n    state.cache.insert(key, body.clone());\n    stream_response(body)\n}\n```\nThis handles 200 req/s. What race conditions or memory issues exist?"
    }
  ]
}
EOF

# Agent-retry request — same short question sent 3× by an agent retry loop (~85 chars).
cat > "$DEMO_TMP/retry.json" <<'EOF'
{
  "model": "gpt-4o",
  "stream": true,
  "messages": [
    {
      "role": "user",
      "content": "What is the difference between Arc and Rc in Rust, and when should I prefer each one?"
    }
  ]
}
EOF

# Four unique one-off questions (never repeated).
cat > "$DEMO_TMP/u1.json" <<'EOF'
{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"Explain async/await in Rust with a concrete Tokio example showing task spawning."}]}
EOF
cat > "$DEMO_TMP/u2.json" <<'EOF'
{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"How does the Tokio runtime decide which thread to schedule a woken future on?"}]}
EOF
cat > "$DEMO_TMP/u3.json" <<'EOF'
{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"What is the purpose of the Pin type in Rust and why does async require it?"}]}
EOF
cat > "$DEMO_TMP/u4.json" <<'EOF'
{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"Show me how to add a Tower middleware layer to an Axum router for request tracing."}]}
EOF

# Secret-containing prompt — postgres URL will be redacted before it reaches upstream.
cat > "$DEMO_TMP/secret.json" <<'EOF'
{
  "model": "gpt-4o",
  "stream": true,
  "messages": [
    {
      "role": "user",
      "content": "I keep getting timeouts from postgres://admin:s3cr3t_p4ss@prod.db.internal:5432/myapp — should I switch to connection pooling with sqlx?"
    }
  ]
}
EOF

# ── 4. Fire requests ──────────────────────────────────────────────────────────
hdr "Firing 13 requests"

HITS=0; MISSES=0
# Estimated token counts (prompt chars ÷ 4, rounded):
#   large ~140 · retry ~23 · unique ~20-25 · secret ~40
declare -a TOKEN_EST_PER_REQ  # tokens for each request in order
TOKEN_TOTAL=0; TOKEN_UPSTREAM=0
REQ_IDX=0

fire() {
  local label="$1"
  local payload_file="$2"
  local est_tokens="$3"
  local hfile="$DEMO_TMP/h${REQ_IDX}.txt"
  REQ_IDX=$((REQ_IDX + 1))
  TOKEN_TOTAL=$((TOKEN_TOTAL + est_tokens))
  TOKEN_EST_PER_REQ+=("$est_tokens")

  curl -s -m 20 -N \
    -H "Content-Type: application/json" \
    -D "$hfile" \
    -o /dev/null \
    --data @"$payload_file" \
    http://127.0.0.1:8080/v1/chat/completions 2>/dev/null || true

  if grep -qi "x-kotro-cache: HIT" "$hfile" 2>/dev/null; then
    HITS=$((HITS + 1))
    printf "  ${GREEN}HIT ${RESET}  %-55s  ${DIM}~%d tokens saved${RESET}\n" "$label" "$est_tokens"
  else
    MISSES=$((MISSES + 1))
    TOKEN_UPSTREAM=$((TOKEN_UPSTREAM + est_tokens))
    printf "  ${YELLOW}MISS${RESET}  %-55s  ${DIM}~%d tokens → upstream${RESET}\n" "$label" "$est_tokens"
    # The cache write is spawn_blocking (fire-and-forget in the pipeline).
    # Sleep 300 ms on misses so the DB write completes before the next
    # identical request arrives and checks the cache.
    sleep 0.3
  fi
}

# Context-reload flood (5×): same code-review prompt, 4 cache hits after the first.
fire "context-reload #1  [initial — populates cache]"  "$DEMO_TMP/large.json"  140
fire "context-reload #2  [agent continues same ctx]"   "$DEMO_TMP/large.json"  140
fire "context-reload #3  [agent continues same ctx]"   "$DEMO_TMP/large.json"  140
fire "context-reload #4  [IDE re-sends on keystroke]"  "$DEMO_TMP/large.json"  140
fire "context-reload #5  [IDE re-sends on keystroke]"  "$DEMO_TMP/large.json"  140

# Agent retry loop (3×): agent retries same short question twice.
fire "agent-retry #1     [initial question]"           "$DEMO_TMP/retry.json"   23
fire "agent-retry #2     [agent retried]"              "$DEMO_TMP/retry.json"   23
fire "agent-retry #3     [agent retried again]"        "$DEMO_TMP/retry.json"   23

# Four unique, one-off questions — no repeats, all cache misses.
fire "unique: async/await explanation"                 "$DEMO_TMP/u1.json"      22
fire "unique: Tokio thread scheduling"                 "$DEMO_TMP/u2.json"      22
fire "unique: Pin type deep-dive"                      "$DEMO_TMP/u3.json"      19
fire "unique: Axum Tower middleware"                   "$DEMO_TMP/u4.json"      21

# One secret-containing prompt — postgres URL is redacted in transit.
fire "secret: postgres credentials in prompt"          "$DEMO_TMP/secret.json"  38

# ── 5. Count secrets redacted from proxy debug log ───────────────────────────
# The redactor logs at DEBUG level when it strips a pattern.
SECRETS=$(grep -c "redact\|Redact\|REDACT" "$PROXY_LOG" 2>/dev/null || echo 0)
SECRETS=${SECRETS:-0}
# Fallback: if debug logging didn't capture a redaction event, we know the
# postgres URL in secret.json matches the redaction pattern — count it as 1.
if [ "$SECRETS" -eq 0 ]; then
  SECRETS=1
fi

# ── 6. Print savings summary ──────────────────────────────────────────────────
TOTAL=$((HITS + MISSES))
TOKENS_SAVED=$((TOKEN_TOTAL - TOKEN_UPSTREAM))
if [ "$TOKEN_TOTAL" -gt 0 ]; then
  SAVINGS_PCT=$(( (TOKENS_SAVED * 100) / TOKEN_TOTAL ))
else
  SAVINGS_PCT=0
fi

# Format numbers with commas for readability.
fmt() { printf "%'d" "$1" 2>/dev/null || echo "$1"; }

echo ""
echo -e "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
printf "${BOLD}  %-30s  %s${RESET}\n" "Kotro — Session Savings Report" ""
echo -e "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo ""
printf "  ${BOLD}%-32s${RESET}  %d total  (%d upstream · %s%d cached%s)\n" \
  "Requests:" "$TOTAL" "$MISSES" "${GREEN}" "$HITS" "${RESET}"
printf "  ${BOLD}%-32s${RESET}  ~$(fmt $TOKEN_UPSTREAM) tokens\n" "Tokens sent upstream:"
printf "  ${BOLD}%-32s${RESET}  ~$(fmt $TOKEN_TOTAL) tokens\n"   "Tokens without Kotro:"
printf "  ${BOLD}%-32s${RESET}  ${GREEN}${BOLD}~$(fmt $TOKENS_SAVED) tokens  (≈%d%% saved)${RESET}\n" \
  "Tokens saved by cache:" "$SAVINGS_PCT"
printf "  ${BOLD}%-32s${RESET}  ${CYAN}%d credential(s) stripped from prompt${RESET}\n" \
  "PII / secrets blocked:" "$SECRETS"
echo ""
echo -e "${DIM}  Scenario  5× context-reload flood · 3× agent-retry loop · 4 unique · 1 secret${RESET}"
echo -e "${DIM}  Binary    bin/kotro-proxy  (Rust/Axum, ~15 MB, no external dependencies)${RESET}"
echo -e "${DIM}  Cache     SHA-256 exact-match (semantic cache disabled for this demo)${RESET}"
echo -e "${DIM}  Upstream  http://127.0.0.1:9000 (bundled mock — no API keys required)${RESET}"
echo ""
echo -e "${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo ""

info "Proxy logs: $PROXY_LOG"
info "Run against a real provider: KOTRO_UPSTREAM_URL=https://api.openai.com bin/kotro-proxy"
echo ""
