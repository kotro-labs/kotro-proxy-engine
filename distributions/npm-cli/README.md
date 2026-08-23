# @kotro-labs/proxy-engine

<p align="center">
  <img src="media/icon.png" alt="Kotro" width="72" height="72" />
</p>

npm distribution for the [Kotro Proxy Engine](https://github.com/kotro-labs/kotro-proxy-engine) — a local AI reverse proxy with a streaming prompt-state cache, PII redaction, and context compression for OpenAI and Anthropic SDKs.

## Features

- **Zero-config startup** — precompiled binary with no dependencies
- **Prompt-state SSE cache** — exact-match replay on repeat prompts; `X-Kotro-Cache: HIT` on cache hits
- **Enterprise failover** — dynamic routing on 429/503 upstream errors
- **Operator dashboard** — local UI for observability (`http://127.0.0.1:9090/dashboard`)
- **Isolated telemetry** — `/metrics` binds to loopback by default, separate from LLM traffic
- **Context-aware cache keys** — prevents false cache hits in multi-turn agent loops

## Install

```bash
npm install -g @kotro-labs/proxy-engine
```

## Quick start

```bash
# Real provider traffic - set upstream URL and API key
export KOTRO_UPSTREAM_URL=https://api.openai.com
export KOTRO_UPSTREAM_API_KEY=$OPENAI_API_KEY

kotro-proxy
```


Without `KOTRO_UPSTREAM_URL`, a bare `kotro-proxy` looks idle: the default upstream is a local mock on `:9000` that is **not** started for you. For a no-API-key proof, clone the monorepo and run `make demo-injection` - see [See it yourself (no API key)](https://github.com/kotro-labs/kotro-proxy-engine#see-it-yourself-no-api-key).

The proxy listens on `:8080` by default. Point your IDE or SDK at `http://localhost:8080/v1`.

```bash
curl -N http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{"model":"gpt-4","stream":true,"messages":[{"role":"user","content":"hello"}]}'
```

Cache hits return the `X-Kotro-Cache: HIT` header.

## Architecture

```text
AI client  →  localhost:8080/v1/*     (LLM proxy — may bind 0.0.0.0 in cluster mode)
Operator   →  127.0.0.1:9090/dashboard  (telemetry — loopback only by default)
```

## Configuration

| Variable | Default | Purpose |
|----------|---------|---------|
| `KOTRO_LISTEN_ADDR` | `:8080` | Proxy bind address |
| `KOTRO_UPSTREAM_URL` | `http://127.0.0.1:9000` | Provider base URL |
| `KOTRO_ENABLE_CACHE` | `true` | Prompt-state SSE cache |
| `KOTRO_ENABLE_REDACTION` | `true` | PII guardrail |
| `KOTRO_ENABLE_COMPRESSION` | `true` | Context block dedup |
| `KOTRO_TRUST_UPSTREAM_GATEWAY` | `false` | Honor `X-Tenant-ID` only from trusted proxy CIDRs |

Full documentation: [github.com/kotro-labs/kotro-proxy-engine](https://github.com/kotro-labs/kotro-proxy-engine)

## Other install channels

- **Homebrew:** `brew tap kotro-labs/tap && brew install kotro-proxy`
- **VS Code / Cursor:** [Marketplace extension](https://marketplace.visualstudio.com/items?itemName=kotrolabs.kotro-proxy-engine)
- **GitHub Releases:** [Download binaries](https://github.com/kotro-labs/kotro-proxy-engine/releases)

## License

MIT
