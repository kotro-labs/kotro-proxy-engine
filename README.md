# Kotro

<p align="center">
  <img src="distributions/shared/media/icon.png" alt="Kotro" width="96" height="96" />
</p>

<p align="center">
  <strong>Local Rust LLM proxy for Cursor &amp; Claude Code — stop MCP prompt injection, cut agent token waste.</strong>
</p>

<p align="center">
  One binary on your machine. No SaaS. Sees every tool result <em>before</em> it reaches the model.
</p>

<p align="center">
  <a href="https://github.com/kotro-labs/kotro-proxy-engine/actions/workflows/ci.yml"><img src="https://github.com/kotro-labs/kotro-proxy-engine/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/kotro-labs/kotro-proxy-engine/releases"><img src="https://img.shields.io/github/v/release/kotro-labs/kotro-proxy-engine" alt="Release" /></a>
  <a href="https://www.npmjs.com/package/@kotro-labs/proxy-engine"><img src="https://img.shields.io/npm/v/@kotro-labs/proxy-engine" alt="npm" /></a>
  <a href="https://marketplace.visualstudio.com/items?itemName=kotrolabs.kotro-proxy-engine"><img src="https://img.shields.io/visual-studio-marketplace/v/kotrolabs.kotro-proxy-engine?label=VS%20Code" alt="VS Code Marketplace" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT" /></a>
</p>

<p align="center">
  <a href="docs/launch/assets/exploit-demo-recording.mp4"><strong>▶ 78s demo (narrated)</strong></a>
  ·
  <a href="docs/launch/assets/exploit-demo-recording-silent.mp4">silent</a>
  ·
  <a href="http://127.0.0.1:9090/dashboard">dashboard</a>
  ·
  <a href="docs/launch/exploit-demo.md">exploit guide</a>
</p>

<p align="center">
  <a href="docs/launch/assets/dashboard-injection-demo.png">
    <img src="docs/launch/assets/dashboard-injection-demo.png" alt="Kotro dashboard — injections blocked + savings" width="720" />
  </a>
</p>

### Who this is for

- You run **Cursor**, **Claude Code**, or any **OpenAI / Anthropic-compatible** agent with MCP tools
- You want **injection scan + secret redaction** without sending traffic through a third-party gateway
- You also want **cache / budget / circuit breaker** so runaway agents don’t bill you overnight

### 30-second install

```bash
curl -sL https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/scripts/install.sh | bash
kotro-proxy
# Point your agent base URL at http://127.0.0.1:8080/v1
# Dashboard: http://127.0.0.1:9090/dashboard
```

| Also | |
|------|--|
| **Homebrew** | `brew install kotro-labs/tap/kotro-proxy` |
| **npm** | `npm i -g @kotro-labs/proxy-engine` |
| **Cursor / VS Code** | [Marketplace extension](https://marketplace.visualstudio.com/items?itemName=kotrolabs.kotro-proxy-engine) |

### Without Kotro → with Kotro

| Without | With |
|---------|------|
| Poisoned MCP / tool text rides into the next LLM call | Scan → **warn** header or **HTTP 400** hard-block; dashboard **Detected / Blocked** |
| Retries & identical turns pay full price | Exact-match cache replay (`x-kotro-cache: HIT`) — **~68%** in the savings demo |
| Agent loops burn credits unnoticed | Circuit breaker + optional session token budget (**HTTP 429**) |
| Secrets leave with the prompt | Redacted outbound, restored on the stream |

### See it yourself (no API key)

```bash
git clone https://github.com/kotro-labs/kotro-proxy-engine.git && cd kotro-proxy-engine
make demo-savings      # ~68% savings story + secret redaction
make demo-injection    # warn → HTTP 400 block + security tiles
```

> **Honest path:** Kotro sits on the **HTTP path** to the provider — not raw MCP stdio. When an agent includes poisoned tool/file content in the next `/v1/chat/completions` or `/v1/messages` body, the scanner sees it.

---

## Point your agent at it

### Cursor

1. **Settings → Models**
2. Set **OpenAI Base URL** to `http://localhost:8080/v1`
3. Keep your provider API key
4. Optional: Command Palette → **Kotro: Verify Cache** (MISS then HIT)

### Claude Code

```bash
KOTRO_UPSTREAM_URL=https://api.anthropic.com kotro-proxy &
ANTHROPIC_BASE_URL=http://localhost:8080 claude
```

### OpenAI-compatible (Aider, SDKs, …)

```bash
OPENAI_BASE_URL=http://localhost:8080/v1 your-tool
```

---

## What it does

| Feature | Description |
|--------|-------------|
| **MCP prompt injection scanner** | 14 regex patterns on tool / user text. Warn-by-default; `KOTRO_INJECTION_BLOCK=true` → HTTP **400**. |
| **Secret redaction** | API keys, DB URLs, passwords, PII stripped before the cloud; restored in the stream. |
| **Agent loop circuit breaker** | 3+ identical tool calls → trip (`X-Kotro-Circuit-Open`). |
| **Reasoning budget controller** | Caps Anthropic `thinking.budget_tokens` / OpenAI `max_completion_tokens`. |
| **Streaming prompt-state cache** | Exact-match SSE replay on repeated prompts (redb). |
| **Local semantic cache** | On-device MiniLM (`candle`) for paraphrases — optional, ~26ms. |
| **MCP tool result cache** | TTL by category; writes invalidate reads. |
| **Context compressor** | Strips unchanged MCP schemas / trees across turns. |
| **Per-session token budget** | Hard cap → HTTP **429** + `X-Kotro-Budget-Remaining`. |
| **WASM plugins** | Bring-your-own guardrails (Go / TS / Python → WASM). |
| **OpenTelemetry** | OTLP traces per request. |

---

## Is Kotro the right tool for you?

Kotro is deliberately narrow: a single-binary proxy for **one developer’s** coding-agent traffic. It is not a team-wide multi-provider gateway.

| Tool | Best fit |
|---|---|
| **Kotro** | Local security + efficiency for MCP-native agents; **no third party** in the request path |
| **[LiteLLM](https://github.com/BerriAI/litellm)** | Team/org routing to 100+ providers behind one API |
| **[Portkey](https://github.com/Portkey-AI/gateway)** | Heavier production guardrails / managed options |
| **Hosted gateways** | Zero infra — but a third party sees 100% of traffic |

That “nothing else in the path” property is structural, not a checkbox — a hosted gateway can’t offer it without changing its business model.

### About the 99.3% upstream-token figure

In a published 3-turn codebase eval ([`benchmarks/eval-suite/RESULTS.md`](benchmarks/eval-suite/RESULTS.md)), billed upstream tokens dropped dramatically **because request-shape stability let the *provider’s* prefix cache fire**. Kotro’s local cache *missed* on those turns (new content each time). Treat **68%** / `make demo-savings` as the honest “same day with local cache” story; treat 99.3% as an upstream-prefix interaction, not “local cache alone.”

---

## Install details

| Channel | Command |
|---------|---------|
| **1-Click (macOS/Linux)** | `curl -sL https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/scripts/install.sh \| bash` |
| **Homebrew** | `brew install kotro-labs/tap/kotro-proxy` |
| **npm** | `npm install -g @kotro-labs/proxy-engine` |
| **Docker** | `docker run -p 8080:8080 kotrolabs/kotro-proxy` |
| **Marketplace** | [kotrolabs.kotro-proxy-engine](https://marketplace.visualstudio.com/items?itemName=kotrolabs.kotro-proxy-engine) |
| **Release binary** | [GitHub Releases](https://github.com/kotro-labs/kotro-proxy-engine/releases) |
| **From source** | `cargo install --path rust/kotro-proxy` |

### Verifying releases

Release binaries are signed keylessly via [cosign](https://github.com/sigstore/cosign) + SPDX SBOM:

```bash
cosign verify-blob \
  --certificate kotro-proxy-x86_64-apple-darwin.tar.gz.pem \
  --signature kotro-proxy-x86_64-apple-darwin.tar.gz.sig \
  --certificate-identity-regexp 'https://github.com/kotro-labs/kotro-proxy-engine/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  kotro-proxy-x86_64-apple-darwin.tar.gz
```

`curl | bash` / Homebrew / npm do not run this check automatically — verify the GitHub Release asset when you need that guarantee.

---

## Quick start (manual)

```bash
KOTRO_UPSTREAM_URL=https://api.openai.com kotro-proxy
# or: KOTRO_UPSTREAM_URL=https://api.anthropic.com kotro-proxy
```

Point the IDE/SDK at `http://127.0.0.1:8080/v1`. Dashboard: [http://127.0.0.1:9090/dashboard](http://127.0.0.1:9090/dashboard).

### Smoke curls

```bash
curl -N http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{"model":"gpt-4o","stream":true,"messages":[{"role":"user","content":"hello"}]}'
```

```bash
curl -N http://127.0.0.1:8080/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-3-5-sonnet-20241022","max_tokens":256,"stream":true,"messages":[{"role":"user","content":"hello"}]}'
```

Cache hits return `X-Kotro-Cache: HIT`. Hard-block injections with `KOTRO_INJECTION_BLOCK=true`.

### Aider + local Ollama

```bash
ollama run llama3
KOTRO_UPSTREAM_URL=http://localhost:11434/v1 kotro-proxy &
export ANTHROPIC_API_KEY="dummy"
aider --model anthropic/claude-3-5-sonnet-20241022 --openai-api-base http://localhost:8080/v1
```

---

## Configuration

| Variable | Default | Purpose |
|----------|---------|---------|
| `KOTRO_LISTEN_ADDR` | `:8080` | Proxy bind address |
| `KOTRO_UPSTREAM_URL` | `http://127.0.0.1:9000` | Provider base URL |
| `KOTRO_ENABLE_CACHE` | `true` | Prompt-state SSE cache |
| `KOTRO_ENABLE_VECTOR_CACHE` | `true` | On-device MiniLM layer |
| `KOTRO_ENABLE_REDACTION` | `true` | PII / secret guardrail |
| `KOTRO_ENABLE_COMPRESSION` | `true` | Context dedup |
| `KOTRO_ENABLE_INJECTION_SCAN` | `true` | MCP injection patterns |
| `KOTRO_INJECTION_BLOCK` | `false` | Hard-block → HTTP 400 |
| `KOTRO_CACHE_HIT_DELAY_MS` | `2` | Replay pacing |
| `KOTRO_CACHE_TTL` | `24h` | Entry lifetime (`0` = no expiry) |
| `KOTRO_EVICTION_INTERVAL` | `10m` | Expired-key sweep |
| `KOTRO_ENABLE_METRICS` | `true` | `/metrics` + `/dashboard` |
| `KOTRO_METRICS_ADDR` | `127.0.0.1:9090` | Telemetry bind |
| `KOTRO_DASHBOARD_USD_PER_TOKEN` | `0.000015` | Hero $ estimate rate |
| `KOTRO_OTEL_ENDPOINT` | (empty) | OTLP traces |
| `KOTRO_WASM_PLUGINS` | (empty) | Comma-separated `.wasm` paths |
| `KOTRO_REDIS_URL` | (empty) | Optional shared cache |
| `KOTRO_CACHE_KEY_STRATEGY` | `window_n` | `latest_only` \| `window_n` \| `full_digest` |
| `KOTRO_CACHE_WINDOW_SIZE` | `4` | Turns hashed for `window_n` |
| `KOTRO_PROFILE` | (empty) | `cursor` \| `copilot` \| `continue` |

### Cache key strategies

| Strategy | What is hashed | Recommended for |
|----------|----------------|-----------------|
| **`window_n`** (default) | System + last *N* turns | Production agent loops |
| **`full_digest`** | Entire conversation | Strict / multi-tenant |
| **`latest_only`** | System + latest user | Legacy only — **risky** for multi-turn |

### Profiles

| Profile | Listen | Cache strategy | IDE |
|---------|--------|----------------|-----|
| `cursor` | `:8080` | `window_n` | Cursor |
| `copilot` | `:8080` | `full_digest` | GitHub Copilot |
| `continue` | `:8080` | `window_n` | Continue.dev |

---

## Architecture

```
IDE / Agent  →  kotro-proxy (:8080)
                 ├─ injection scan · redaction · loop / budget
                 ├─ cache · compress · tool cache
                 ├─ /v1/chat/completions
                 ├─ /v1/messages
                 └─ /v1/*  (passthrough)
                        ↓
                 upstream (OpenAI, Anthropic, Ollama, mock, …)
```

Launch docs: [Week 1 distribution](docs/launch/WEEK1-DISTRIBUTION.md) · [Dev.to article (cost → security)](docs/launch/devto-article-3-savings-then-security.md) · [Viral packaging notes](docs/launch/VIRAL-README-PLAYBOOK.md)

## Cancel-storm leak audit

```bash
brew install k6
make cancel-audit
```

## Rust (shipping target)

```bash
cd rust && cargo test && cargo run -p kotro-proxy
```

Architecture map: [docs/RUST-ARCHITECTURE.md](docs/RUST-ARCHITECTURE.md)

The Go tree under `internal/` is frozen at **[v0.1.0-go](https://github.com/kotro-labs/kotro-proxy-engine/releases/tag/v0.1.0-go)** as a behavioral reference — not the runtime you should ship.

## Benchmarks

```bash
make load-test
make eval-suite
```

Results: [benchmarks/eval-suite/RESULTS.md](benchmarks/eval-suite/RESULTS.md). Threat model: [docs/security/THREAT-MODEL.md](docs/security/THREAT-MODEL.md).

## Project layout

```
rust/kotro-proxy/    Active Rust implementation
cmd/mockupstream/    Offline OpenAI + Anthropic SSE mock
docs/launch/         Demo scripts, HN / Dev.to assets
internal/            Frozen Go reference
scripts/             install, demos, benches
```

## License

[MIT](LICENSE) — contributions welcome.
