# Korto Proxy Engine

<p align="center">
  <img src="distributions/shared/media/icon.png" alt="Korto" width="96" height="96" />
</p>

[![CI](https://github.com/ramairwing/kotro-proxy-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/ramairwing/kotro-proxy-engine/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ramairwing/kotro-proxy-engine)](https://github.com/ramairwing/kotro-proxy-engine/releases)
[![npm](https://img.shields.io/npm/v/@kortosystems/proxy-engine)](https://www.npmjs.com/package/@kortosystems/proxy-engine)
[![VS Code Marketplace](https://img.shields.io/visual-studio-marketplace/v/kortosystems.kortolabs-proxy-engine?label=VS%20Code)](https://marketplace.visualstudio.com/items?itemName=kortosystems.kortolabs-proxy-engine)

**The open-source, single-binary local AI proxy** — intercept streaming LLM traffic from OpenAI and Anthropic SDKs, cut token waste, and keep secrets off the wire.

Korto fills the gap between local agent runtimes (Cursor, Claude Code, custom SDK clients) and cloud providers. It is designed as the premier self-hosted alternative to hosted gateways like TokenShift: one binary, no SaaS dependency, full control over cache, redaction, and context compression.

## The Benchmark Proof (99.3% Cost Reduction)

By coordinating local semantic caching with upstream prefix caching (like DeepSeek V4 and Qwen), Korto radically reduces inference costs for heavy agent loops. 

In a standard 3-turn codebase benchmark:
- **Turn 1**: 2042 tokens sent.
- **Turn 2**: Local Proxy Miss → DeepSeek Server Hit (**1920 tokens cached upstream**). *Only 141 tokens billed.*
- **Turn 3**: Local Proxy Miss → DeepSeek Server Hit (**1920 tokens cached upstream**). *Only 159 tokens billed.*

**Total Upstream Billed Tokens: ~99.3% Reduction.**

## What it does

| Feature | Description |
|--------|-------------|
| **Streaming semantic cache** | Captures complete SSE streams on miss; replays on identical prompt state (system + latest user + model). |
| **Privacy guardrail** | Redacts secrets before upstream; restores placeholders in streaming responses. |
| **Context compressor** | Strips unchanged MCP schemas / directory trees across turns. |
| **Universal provider support** | OpenAI-compatible APIs (DeepSeek, Groq, Ollama, etc.) and Anthropic `POST /v1/messages`. |
| **Offline test harness** | Mock upstream simulates chunked OpenAI + Anthropic SSE without network. |
| **Load benchmarks** | k6 and vegeta scripts for cache hit/miss and mixed workloads. |

## Install

| Channel | Command |
|---------|---------|
| **Docker** | `docker-compose up` (mock upstream + Rust proxy) |
| **npm** | `npm install -g @kortosystems/proxy-engine` → `kortolabs-proxy` |
| **Homebrew** | `brew tap ramairwing/tap && brew trust ramairwing/tap && brew install kortolabs-proxy` |
| **VS Code / Cursor** | [Marketplace extension](https://marketplace.visualstudio.com/items?itemName=kortosystems.kortolabs-proxy-engine) (status bar + dashboard link) |
| **GitHub Release** | [Download binary](https://github.com/ramairwing/kotro-proxy-engine/releases) for your platform |
| **From source** | `make build` or `cd rust && cargo run -p korto-proxy` |

Registry publish runs automatically on `v*` tags when `NPM_TOKEN` and `VSCE_PAT` secrets are configured. See [distributions/SECRETS.md](distributions/SECRETS.md).

## Quick start

```bash
git clone git@github.com:ramairwing/kotro-proxy-engine.git
cd korto-proxy-engine

make build
make test

# Terminal A: mock provider (:9000)
bin/mock-upstream

# Terminal B: proxy (:8080)
KORTO_UPSTREAM_URL=http://127.0.0.1:9000 bin/kortolabs-proxy

# Or both:
make dev
```

Point your IDE or SDK at `http://localhost:8080/v1`.

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

Cache hits return `X-KortoLabs-Cache: HIT`.

Local dashboard: [http://127.0.0.1:9090/dashboard](http://127.0.0.1:9090/dashboard) (requires `KORTO_ENABLE_METRICS=true`).

## Configuration

| Variable | Default | Purpose |
|----------|---------|---------|
| `KORTO_LISTEN_ADDR` | `:8080` | Proxy bind address |
| `KORTO_UPSTREAM_URL` | `http://127.0.0.1:9000` | Provider base URL |
| `KORTO_ENABLE_CACHE` | `true` | Semantic SSE cache |
| `KORTO_ENABLE_REDACTION` | `true` | Local PII guardrail |
| `KORTO_ENABLE_COMPRESSION` | `true` | Context deduplication |
| `KORTO_CACHE_HIT_DELAY_MS` | `2` | Replay pacing on cache hits |
| `KORTO_CACHE_TTL` | `24h` | Cache entry lifetime (`0` disables expiry) |
| `KORTO_EVICTION_INTERVAL` | `10m` | Background sweep for expired keys |
| `KORTO_ENABLE_PPROF` | `false` | Expose `/debug/pprof` for leak audits |
| `KORTO_ENABLE_METRICS` | `true` | Expose `/metrics` and `/dashboard` on `KORTO_METRICS_ADDR` (default `127.0.0.1:9090`) |
| `KORTO_METRICS_ADDR` | `127.0.0.1:9090` | Isolated telemetry bind address |
| `KORTO_CACHE_KEY_STRATEGY` | `window_n` | Cache key material: `latest_only`, `window_n`, `full_digest` |
| `KORTO_CACHE_WINDOW_SIZE` | `4` | Trailing non-system turns hashed when strategy is `window_n` |

### Cache key strategies

| Strategy | What is hashed | Recommended for |
|----------|----------------|-----------------|
| **`window_n`** (default) | System prompt + last *N* user/assistant/tool turns | **Production agent loops** — balances hit rate and correctness |
| **`full_digest`** | Entire conversation JSON | **Shared multi-tenant** or strict deterministic pipelines |
| **`latest_only`** | System + latest user text only | Legacy compatibility only — **risky** for multi-turn agents |

`latest_only` can return a cache hit when two agent sessions share the same final user phrase but different tool outputs in between (silent state corruption). Prefer `window_n` or `full_digest` in production.

Prometheus exposes the active strategy as `korto_cache_key_strategy{strategy,window_size}`.

### Deployment Profiles & IDE Presets

You can use the `KORTO_PROFILE` environment variable for zero-friction setup:

| Profile | Listen | Cache strategy | Recommended IDE |
|---------|--------|----------------|-----------------|
| `cursor` | `:8080` | `window_n` | **Cursor** (Automatically handles Composer context) |
| `copilot` | `:8080` | `full_digest` | **GitHub Copilot** (Disables redaction for strict autocompletion) |
| `continue` | `:8080` | `window_n` | **Continue.dev** |

For advanced control:

| Profile | Listen | Cache strategy | Scope / trust |
|---------|--------|----------------|---------------|
| **Local dev** | `:8080` | `window_n` | Default credential-derived scope |
| **Trusted gateway** | `0.0.0.0:8080` | `window_n` | `KORTO_TRUST_UPSTREAM_GATEWAY=true` + `KORTO_TRUSTED_PROXY_CIDRS` |
| **Shared multi-tenant** | `0.0.0.0:8080` | `full_digest` | Gateway headers + trusted proxy CIDRs; telemetry on loopback only |

## Cancel-storm leak audit (k6 + pprof)

Verifies zero goroutine leak after mass mid-stream client disconnects.

```bash
brew install k6
make cancel-audit

# Full storm: 500 parallel agents for 30s
K6_VUS=500 K6_DURATION=30s make cancel-audit
```

Requires `KORTO_ENABLE_PPROF=true` (set automatically by `run_audit.sh`). Pass criteria: post-stress goroutine count within ±5 of baseline.

## Rust Phase 2

Go Phase 1 is the behavioral reference. The Rust port lives in `rust/`:

```bash
cd rust && cargo test && cargo run -p korto-proxy
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
IDE / SDK  →  kortolabs-proxy (:8080)
                 ├─ /v1/chat/completions  (intercept: cache · redact · compress)
                 ├─ /v1/messages          (intercept: cache · redact · compress)
                 └─ /v1/*                 (passthrough)
                        ↓
                 upstream provider (OpenAI, Anthropic, mock, …)
```

## Project layout

```
cmd/proxy/           Main proxy binary
cmd/mockupstream/    Offline OpenAI + Anthropic SSE server
internal/cache/      bbolt semantic cache
internal/compressor/ Context block dedup
internal/guardrail/  Secret redaction
internal/models/     OpenAI + Anthropic request types
internal/proxy/      Handlers, SSE interceptor pipeline
internal/sse/        Frame parser (OpenAI data: + Anthropic event:)
scripts/bench/       k6 / vegeta load tests
```

## License

Open source — contributions welcome.
