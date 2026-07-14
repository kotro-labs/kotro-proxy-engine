# Kotro Proxy Engine

<p align="center">
  <img src="distributions/shared/media/icon.png" alt="Kotro" width="96" height="96" />
</p>

[![CI](https://github.com/kotro-labs/kotro-proxy-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/kotro-labs/kotro-proxy-engine/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/kotro-labs/kotro-proxy-engine)](https://github.com/kotro-labs/kotro-proxy-engine/releases)
[![npm](https://img.shields.io/npm/v/@kotro-labs/proxy-engine)](https://www.npmjs.com/package/@kotro-labs/proxy-engine)
[![VS Code Marketplace](https://img.shields.io/visual-studio-marketplace/v/kotrolabs.kotro-proxy-engine?label=VS%20Code)](https://marketplace.visualstudio.com/items?itemName=kotrolabs.kotro-proxy-engine)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**The local security and efficiency layer for MCP-native agentic AI** — intercept streaming LLM traffic from OpenAI and Anthropic SDKs, block prompt injection from tool results, keep secrets off the wire, and cut token waste. One binary, no SaaS, no third party ever in the request path.

As MCP-native agents (Cursor, Claude Code, custom SDK clients) pull in tool results from files, git, search, and external APIs, those results become the largest new attack surface in the stack — a single malicious tool result can hijack the agent's next action. Kotro sits between your agent and the cloud provider and enforces a security policy your LLM provider cannot: it sees every message, every tool result, and every secret before they leave your machine.

## Is Kotro the right tool for you?

Kotro is deliberately narrow: a single-binary, zero-dependency proxy for one developer's coding-agent traffic. It is not trying to be a team-wide LLM gateway — for that, the tools below are more mature and better fits.

| Tool | Deployment | Best fit |
|---|---|---|
| **Kotro** | Self-hosted, single binary, zero external services (no Redis, no vector DB, no Postgres) | One developer's machine running MCP-native agents — you want a local security layer (injection scanner, secret redaction, circuit breaker) and efficiency layer (cache, compressor, budget) without any third party in the request path. |
| **[LiteLLM](https://github.com/BerriAI/litellm)** | Self-hosted Python proxy | A team or org routing to 100+ providers behind one OpenAI-compatible API, with a large ecosystem and community behind it. |
| **[Portkey](https://github.com/Portkey-AI/gateway)** | Self-hosted (Apache 2.0) or managed cloud | A team that needs production guardrails (PII/jailbreak/prompt-injection detection) and real embedding-based semantic caching out of the box, at the cost of a heavier deployment. |
| **TokenShift** and similar hosted gateways | Managed SaaS | Teams that want zero infrastructure to run themselves and are comfortable with a third-party operator seeing 100% of their traffic in exchange for that convenience. |

If you're evaluating infrastructure that will see your API keys and your code, that "no third party ever sees your traffic" property is the one thing here that's structural, not a feature checkbox — it's true of Kotro by construction (there's nothing else in the request path) and isn't something a hosted gateway can offer without changing its own business model.

## The Benchmark Proof (99.3% Upstream Token Reduction)

By preserving request-shape stability for upstream prefix caching (DeepSeek V4, Qwen, and similar) and layering a local prompt-state cache on top for exact repeats, Kotro reduces inference costs for heavy agent loops.

In a standard 3-turn codebase benchmark (full data in [`benchmarks/eval-suite/RESULTS.md`](benchmarks/eval-suite/RESULTS.md)):
- **Turn 1**: 2042 tokens sent.
- **Turn 2**: Local proxy cache miss (new turn content) → forwarded upstream → DeepSeek's own prefix cache hit (**1920 tokens cached server-side**). *Only 141 tokens billed.*
- **Turn 3**: Local proxy cache miss → DeepSeek prefix cache hit again. *Only 159 tokens billed.*

**Total upstream billed tokens: ~99.3% reduction in this benchmark.**

**Read this number precisely:** in every recorded turn above, Kotro's own local cache *missed* — each turn had new content, so there was nothing to replay locally. The reduction shown here is upstream provider prefix caching doing the work; Kotro's contribution in this specific benchmark is keeping the request shape stable so that upstream caching can fire cleanly, not a local cache hit. Kotro's local cache adds a second, independent savings layer on genuinely repeated prompts (retries, shared fixtures, parallel agent runs hitting the same turn) with zero upstream round-trip — that scenario isn't yet represented in the published eval suite, and we're adding a repeated-prompt fixture so the local-cache contribution can be measured and reported on its own.

## What it does

| Feature | Description |
|--------|-------------|
| **MCP prompt injection scanner** | Scans every tool result and user message for prompt injection patterns (14 regex rules) before forwarding to the LLM. Warn-by-default; set `KOTRO_INJECTION_BLOCK=true` to hard-block. |
| **Secret redaction** | Strips API keys, DB URLs, passwords, and PII from requests before they leave your machine; restores placeholders in streaming responses so your agent still sees the right values. |
| **Agent loop circuit breaker** | Detects 3+ identical tool calls in one conversation window and opens the circuit (`X-Kotro-Circuit-Open` header), preventing runaway billing from stuck agent loops. |
| **Reasoning model budget controller** | Caps `thinking.budget_tokens` (Anthropic) or `max_completion_tokens` (OpenAI o1/o3) per request. Set `KOTRO_MAX_THINKING_TOKENS`; set `KOTRO_REASONING_BLOCK=true` to block reasoning models entirely. |
| **Streaming prompt-state cache** | Captures complete SSE streams on miss; replays on exact-match prompt state (system + latest user + model). Zero upstream round-trip on repeated prompts. |
| **Local semantic cache (Rust engine)** | Layers embedding-based fuzzy matching on top of the exact-match cache — [`all-MiniLM-L6-v2`](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) runs on-device via `candle`. Paraphrased prompts hit the cache with no embedding API call. Adds ~26-28ms per request. |
| **MCP tool result cache** | Caches `read_file`, `git_status`, search results by `(scope, tool_name, args)` with per-category TTLs (read=30s / status=5m / search=1h). Write operations auto-invalidate stale reads for the same path. |
| **Context compressor** | Strips unchanged MCP schemas and directory trees across turns, reducing tokens sent on every non-first turn. |
| **Per-session token budget** | Hard cap on tokens per scope per session (`KOTRO_SESSION_TOKEN_BUDGET`). Returns HTTP 429 with `X-Kotro-Budget-Remaining` header before the LLM call is made. |
| **Universal provider support** | OpenAI-compatible APIs (DeepSeek, Groq, Ollama, etc.) and Anthropic `POST /v1/messages`. |
| **Offline test harness** | Mock upstream simulates chunked OpenAI + Anthropic SSE without network. |
| **WASM Plugins** | Bring-your-own security guardrails written in Go, TS, or Python. Dynamically loaded on boot. |
| **OpenTelemetry** | Export native distributed traces (OTLP/HTTP) for every request to Datadog, New Relic, Jaeger, etc. |
| **Shared Team Cache** | Optionally configure `KOTRO_REDIS_URL` to seamlessly switch from embedded local cache to shared Redis. |

## Install

| Channel | Command |
|---------|---------|
| **1-Click Install (macOS/Linux)** | `curl -sL https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/scripts/install.sh \| bash` |
| **Docker** | `docker run -p 3000:3000 kotrolabs/kotro-proxy` |
| **npm** | `npm install -g @kotro-labs/proxy-engine` → `kotro-proxy` |
| **Homebrew** | `brew install kotro-labs/tap/kotro` |
| **VS Code / Cursor** | [Marketplace extension](https://marketplace.visualstudio.com/items?itemName=kotrolabs.kotro-proxy-engine) |
| **GitHub Release** | [Download binary](https://github.com/kotro-labs/kotro-proxy-engine/releases) |
| **From source** | `cargo install --path rust/kotro-proxy` |

Registry publish runs automatically on `v*` tags when `NPM_TOKEN` and `VSCE_PAT` secrets are configured. Marketplace uses [marketplace-publish.yml](.github/workflows/marketplace-publish.yml) (see [distributions/MARKETPLACE-AUTOMATION.md](distributions/MARKETPLACE-AUTOMATION.md)).

### Verifying releases

Every GitHub Release binary is signed keylessly via [cosign](https://github.com/sigstore/cosign) — the workflow proves it built the artifact using its own GitHub Actions identity (via Sigstore's public Fulcio/Rekor infrastructure), with no private key for anyone to manage or leak. Each release also ships an SPDX SBOM. To verify a downloaded binary before running it:

```bash
# Download the binary, its .sig, and its .pem alongside it (all in the same GitHub Release)
cosign verify-blob \
  --certificate kotro-proxy-x86_64-apple-darwin.tar.gz.pem \
  --signature kotro-proxy-x86_64-apple-darwin.tar.gz.sig \
  --certificate-identity-regexp 'https://github.com/kotro-labs/kotro-proxy-engine/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  kotro-proxy-x86_64-apple-darwin.tar.gz
```

A successful verification confirms the binary was built by this repository's release workflow and hasn't been modified since. The `curl | bash` and Homebrew/npm channels above don't currently run this check automatically — if you need that guarantee, verify the GitHub Release asset directly rather than one of the wrapped installers, or open an issue if automatic verification in those channels matters for your use case.

## Plug-and-Play Guides

### Cursor Integration (Cut API bills in half)
1. In Cursor, open **Settings → Models**.
2. Set the `OpenAI Base URL` to `http://localhost:3000/v1`.
3. Set your OpenAI/Anthropic API Key.
4. Enjoy prompt-state caching and AST pruning out of the box!

### Aider with Local Ollama (Universal Translation)
Kotro automatically translates protocols. You can use Anthropic-native tools with local OpenAI-compatible models!
1. Start your local Ollama: `ollama run llama3`.
2. Start Kotro, pointing upstream to Ollama: `KOTRO_UPSTREAM_URL=http://localhost:11434/v1 kotro`
3. Run Aider:
```bash
export ANTHROPIC_API_KEY="dummy"
aider --model anthropic/claude-3-5-sonnet-20241022 --openai-api-base http://localhost:3000/v1
```

## Quick start

```bash
# Terminal A: Start Proxy
kotro
```

Point your IDE or SDK at `http://localhost:3000/v1`. View your savings dashboard at `http://localhost:3000/`.

### OpenAI-Compatible (DeepSeek, Groq, Ollama)

```bash
curl -N http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{"model":"gpt-4","stream":true,"messages":[{"role":"user","content":"hello"}]}'
```

### Anthropic (streaming)

```bash
curl -N http://127.0.0.1:8080/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-3-5-sonnet-20241022","max_tokens":256,"stream":true,"messages":[{"role":"user","content":"hello"}]}'
```

Cache hits return `X-Kotro-Cache: HIT`.

Local dashboard: [http://127.0.0.1:9090/dashboard](http://127.0.0.1:9090/dashboard) (requires `KOTRO_ENABLE_METRICS=true`).

## Configuration

| Variable | Default | Purpose |
|----------|---------|---------|
| `KOTRO_LISTEN_ADDR` | `:8080` | Proxy bind address |
| `KOTRO_UPSTREAM_URL` | `http://127.0.0.1:9000` | Provider base URL |
| `KOTRO_ENABLE_CACHE` | `true` | Prompt-state SSE cache |
| `KOTRO_ENABLE_VECTOR_CACHE` | `true` | Local semantic (embedding) cache layer — Rust engine only |
| `KOTRO_ENABLE_REDACTION` | `true` | Local PII guardrail |
| `KOTRO_ENABLE_COMPRESSION` | `true` | Context deduplication |
| `KOTRO_CACHE_HIT_DELAY_MS` | `2` | Replay pacing on cache hits |
| `KOTRO_CACHE_TTL` | `24h` | Cache entry lifetime (`0` disables expiry) |
| `KOTRO_EVICTION_INTERVAL` | `10m` | Background sweep for expired keys |
| `KOTRO_ENABLE_PPROF` | `false` | Expose `/debug/pprof` for leak audits |
| `KOTRO_ENABLE_METRICS` | `true` | Expose `/metrics` and `/dashboard` on `KOTRO_METRICS_ADDR` (default `127.0.0.1:9090`) |
| `KOTRO_METRICS_ADDR` | `127.0.0.1:9090` | Isolated telemetry bind address |
| `KOTRO_OTEL_ENDPOINT` | (empty) | OpenTelemetry OTLP endpoint (e.g. `http://localhost:4318/v1/traces`) |
| `KOTRO_WASM_PLUGINS` | (empty) | Comma-separated paths to `.wasm` files to load into the request pipeline |
| `KOTRO_REDIS_URL` | (empty) | Redis URL (e.g. `redis://127.0.0.1:6379/`) for shared team caching |
| `KOTRO_CACHE_KEY_STRATEGY` | `window_n` | Cache key material: `latest_only`, `window_n`, `full_digest` |
| `KOTRO_CACHE_WINDOW_SIZE` | `4` | Trailing non-system turns hashed when strategy is `window_n` |

### Cache key strategies

| Strategy | What is hashed | Recommended for |
|----------|----------------|-----------------|
| **`window_n`** (default) | System prompt + last *N* user/assistant/tool turns | **Production agent loops** — balances hit rate and correctness |
| **`full_digest`** | Entire conversation JSON | **Shared multi-tenant** or strict deterministic pipelines |
| **`latest_only`** | System + latest user text only | Legacy compatibility only — **risky** for multi-turn agents |

`latest_only` can return a cache hit when two agent sessions share the same final user phrase but different tool outputs in between (silent state corruption). Prefer `window_n` or `full_digest` in production.

Prometheus exposes the active strategy as `kotro_cache_key_strategy{strategy,window_size}`.

### Deployment Profiles & IDE Presets

You can use the `KOTRO_PROFILE` environment variable for zero-friction setup:

| Profile | Listen | Cache strategy | Recommended IDE |
|---------|--------|----------------|-----------------|
| `cursor` | `:8080` | `window_n` | **Cursor** (Automatically handles Composer context) |
| `copilot` | `:8080` | `full_digest` | **GitHub Copilot** (strict full-context cache keys) |
| `continue` | `:8080` | `window_n` | **Continue.dev** |

For advanced control:

| Profile | Listen | Cache strategy | Scope / trust |
|---------|--------|----------------|---------------|
| **Local dev** | `:8080` | `window_n` | Default credential-derived scope |
| **Trusted gateway** | `0.0.0.0:8080` | `window_n` | `KOTRO_TRUST_UPSTREAM_GATEWAY=true` + `KOTRO_TRUSTED_PROXY_CIDRS` |
| **Shared multi-tenant** | `0.0.0.0:8080` | `full_digest` | Gateway headers + trusted proxy CIDRs; telemetry on loopback only |

## Cancel-storm leak audit (k6 + pprof)

Verifies zero goroutine leak after mass mid-stream client disconnects.

```bash
brew install k6
make cancel-audit

# Full storm: 500 parallel agents for 30s
K6_VUS=500 K6_DURATION=30s make cancel-audit
```

Requires `KOTRO_ENABLE_PPROF=true` (set automatically by `run_audit.sh`). Pass criteria: post-stress goroutine count within ±5 of baseline.

## Go Reference Implementation (Frozen at v0.1.0-go)

The `internal/` directory contains the original Go implementation, tagged **[v0.1.0-go](https://github.com/kotro-labs/kotro-proxy-engine/releases/tag/v0.1.0-go)** and frozen. It is preserved as a behavioral reference — the Rust implementation is what you should run.

> **No new features will land in Go.** Bug reports and PRs for `internal/` will not be accepted. If you're reading the Go code, treat it as an annotated spec for the Rust port.

## Rust implementation

```bash
cd rust && cargo test && cargo run -p kotro-proxy
```

Architecture map: [docs/RUST-ARCHITECTURE.md](docs/RUST-ARCHITECTURE.md)

## Benchmarks

Install [k6](https://k6.io/): `brew install k6`

```bash
chmod +x scripts/bench/run.sh
make load-test          # all scenarios
make load-test SCENARIO=hit
make eval-suite         # full ROI dashboard → benchmarks/eval-suite/RESULTS.md
```

Scenarios: `miss`, `hit`, `anthropic`, `mixed`, `all`.

Eval suite results and methodology: [benchmarks/eval-suite/RESULTS.md](benchmarks/eval-suite/RESULTS.md). Roadmap and security docs: [docs/roadmap/90-DAY-ROADMAP.md](docs/roadmap/90-DAY-ROADMAP.md), [docs/security/THREAT-MODEL.md](docs/security/THREAT-MODEL.md).

Vegeta alternative:

```bash
go install github.com/tsenart/vegeta@latest
bash scripts/bench/vegeta.sh
```

Go micro-benchmarks:

```bash
make bench
```

## Architecture

```
IDE / SDK  →  kotro-proxy (:8080)
                 ├─ /v1/chat/completions  (intercept: cache · redact · compress)
                 ├─ /v1/messages          (intercept: cache · redact · compress)
                 └─ /v1/*                 (passthrough)
                        ↓
                 upstream provider (OpenAI, Anthropic, mock, …)
```

## Project layout

```
rust/kotro-proxy/    Active Rust implementation (run this)
cmd/proxy/           Go proxy binary (frozen — reference only)
cmd/mockupstream/    Offline OpenAI + Anthropic SSE server
internal/            Go reference implementation (frozen at v0.1.0-go)
  cache/             bbolt prompt-state cache
  compressor/        Context block dedup
  guardrail/         Secret redaction
  models/            OpenAI + Anthropic request types
  proxy/             Handlers, SSE interceptor pipeline
  sse/               Frame parser (OpenAI data: + Anthropic event:)
scripts/bench/       k6 / vegeta load tests
```

## License

[MIT](LICENSE) — contributions welcome.
