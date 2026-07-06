# KortoLabs Proxy Engine

<!-- Marketplace README images must use absolute GitHub raw URLs: vsce rewrites
     relative paths to repo-root /media/* which does not exist. -->

<p align="center">
  <img src="https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/distributions/vscode-extension/media/icon.png" alt="Korto" width="96" height="96" />
</p>

Transparent **IDE sidecar** for the [Kotro Proxy Engine](https://github.com/kotro-labs/kotro-proxy-engine) — a local LLM gateway with streaming semantic cache, PII redaction, and context compression for **OpenAI** and **Anthropic** APIs.

Works in **VS Code**, **Cursor**, and other VS Code–compatible editors.

## Features

- **Zero-config sidecar** — spawns the native proxy binary on startup
- **Semantic SSE cache** — faster repeat prompts; `X-KortoLabs-Cache: HIT` on cache hits
- **Status bar** — live cache result and compressor bytes saved (polls every 5s)
- **Operator dashboard** — open from the status bar (`http://127.0.0.1:9090/dashboard`)
- **Isolated telemetry** — `/metrics` and `/dashboard` bind to loopback by default, separate from LLM traffic
- **Context-aware cache keys** — `window_n` strategy prevents false cache hits in multi-turn agent loops

## Screenshots

### Status bar

![Kotro status bar showing cache hit and bytes saved](https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/distributions/vscode-extension/media/status-bar.png)

### Dashboard

![Kotro proxy operator dashboard](https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/distributions/vscode-extension/media/dashboard.png)

## Install

1. Install from the Marketplace (**Install** above), or:
   ```bash
   code --install-extension kortosystems.kortolabs-proxy-engine
   ```
2. Reload the window. The sidecar starts automatically.
3. Point your AI client at `http://localhost:8080/v1` (OpenAI-compatible base URL).

## Quick start (Cursor / VS Code)

1. Set your provider API key in the environment or your agent config.
2. Configure the extension (optional):

   | Setting | Default | Maps to |
   |---------|---------|---------|
   | `kortosystems.listenAddr` | `:8080` | `KORTO_LISTEN_ADDR` |
   | `kortosystems.metricsAddr` | `127.0.0.1:9090` | `KORTO_METRICS_ADDR` |
   | `kortosystems.upstreamUrl` | `https://api.openai.com` | `KORTO_UPSTREAM_URL` |
   | `kortosystems.enableCache` | `true` | `KORTO_ENABLE_CACHE` |
   | `kortosystems.enableRedaction` | `true` | `KORTO_ENABLE_REDACTION` |
   | `kortosystems.enableCompression` | `true` | `KORTO_ENABLE_COMPRESSION` |
   | `kortosystems.enableMetrics` | `true` | `KORTO_ENABLE_METRICS` |

3. Click the **Kotro** item in the status bar to open the dashboard.

## Verify it works (2 minutes)

The extension **starts** the proxy. Your IDE must **send API traffic** to it.

| Step | Action | Success signal |
|------|--------|----------------|
| 1 | **Cmd+Shift+P** → **Korto: Verify Cache** | Notification: `MISS` then `HIT` |
| 2 | Open dashboard (`http://127.0.0.1:9090/dashboard`) | Recent Traffic shows `miss` then `hit` on `/v1/chat/completions` |
| 3 | (Optional) Cursor **Settings → Models** → OpenAI Base URL = `http://localhost:8080/v1` | Chat traffic appears in dashboard |

**Common mistakes**

- Reading the **chat reply** — that is the model answer, not proxy logs.
- Opening `http://localhost:8080/v1/` in a browser — API only; shows `BYPASS`, not cache.
- Using **Korto: Show Proxy Logs** for HIT/MISS — that channel shows startup lines only; use Verify Cache or the dashboard.

## Commands

| Command | Description |
|---------|-------------|
| **Korto: Verify Cache** | Sends two identical test requests; confirms cache HIT |
| **Korto: Connect Cursor** | Wizard for routing Cursor BYOK chat through the proxy |
| **Korto: Setup Continue.dev Config** | Adds Korto to `~/.continue/config.json` |
| **Korto: Open Dashboard** | Opens the local operator UI |
| **Korto: Show Proxy Logs** | Opens the extension output channel (startup / errors) |

## Architecture

```
IDE agent  →  localhost:8080/v1/*     (LLM proxy — may bind 0.0.0.0 in cluster mode)
Operator   →  127.0.0.1:9090/dashboard  (telemetry — loopback only by default)
```

## Other install channels

- **npm:** `npm install -g @kortosystems/proxy-engine`
- **Homebrew:** `brew tap kotro-labs/tap && brew install kortolabs-proxy`
- **GitHub Releases:** [kotro-labs/kotro-proxy-engine](https://github.com/kotro-labs/kotro-proxy-engine/releases)

## Documentation

Full engine docs, threat model, and observability spec: [github.com/kotro-labs/kotro-proxy-engine](https://github.com/kotro-labs/kotro-proxy-engine)

## License

MIT — [Kortosystems](https://marketplace.visualstudio.com/publishers/kortosystems)
