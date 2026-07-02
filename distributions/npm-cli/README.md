# @kortosystems/proxy-engine

npm distribution for the [Korto Proxy Engine](https://github.com/ramairwing/kotro-proxy-engine) — a local AI reverse proxy with streaming semantic cache, PII redaction, and context compression for OpenAI and Anthropic SDKs.

## Install

```bash
npm install -g @kortosystems/proxy-engine
```

## Quick start

```bash
# Point at your provider (default upstream is local mock on :9000)
export KORTO_UPSTREAM_URL=https://api.openai.com

kortolabs-proxy
```

The proxy listens on `:8080` by default. Point your IDE or SDK at `http://localhost:8080/v1`.

```bash
curl -N http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{"model":"gpt-4","stream":true,"messages":[{"role":"user","content":"hello"}]}'
```

Cache hits return the `X-KortoLabs-Cache: HIT` header.

## Configuration

| Variable | Default | Purpose |
|----------|---------|---------|
| `KORTO_LISTEN_ADDR` | `:8080` | Proxy bind address |
| `KORTO_UPSTREAM_URL` | `http://127.0.0.1:9000` | Provider base URL |
| `KORTO_ENABLE_CACHE` | `true` | Semantic SSE cache |
| `KORTO_ENABLE_REDACTION` | `true` | PII guardrail |
| `KORTO_ENABLE_COMPRESSION` | `true` | Context block dedup |
| `KORTO_TRUST_UPSTREAM_GATEWAY` | `false` | Honor `X-Tenant-ID` only from trusted proxy CIDRs |

Full documentation: [github.com/ramairwing/kotro-proxy-engine](https://github.com/ramairwing/kotro-proxy-engine)

## Other install channels

- **Homebrew:** `brew tap ramairwing/tap && brew install kortolabs-proxy`
- **VS Code / Cursor:** [Marketplace extension](https://marketplace.visualstudio.com/items?itemName=kortosystems.kortolabs-proxy-engine)
- **GitHub Releases:** [Download binaries](https://github.com/ramairwing/kotro-proxy-engine/releases)

## License

MIT
