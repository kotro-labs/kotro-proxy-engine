# Show HN: Korto Proxy Engine v0.1.0

**Title (paste into HN):**

> Show HN: Local LLM proxy with semantic SSE cache, PII redaction, and cancel-storm leak audit

**URL:** https://github.com/kotro-labs/kotro-proxy-engine

---

## Post body (HN comment / first reply)

I built Korto Proxy Engine — a single-binary local reverse proxy that sits between IDE agents (Cursor, Claude Code, custom SDKs) and OpenAI/Anthropic.

**Problem:** Hosted AI gateways add latency, cost, and a SaaS dependency. Generic proxies don't understand streaming SSE semantics, so cache hits still feel like network calls, and client disconnects leak goroutines.

**What it does:**

- Intercepts `POST /v1/chat/completions` (OpenAI) and `POST /v1/messages` (Anthropic)
- On cache miss: tees the upstream SSE stream, redacts secrets before upstream, compresses repeated context blocks
- On cache hit: replays the full captured stream locally (`X-KortoLabs-Cache: HIT`)
- HTTP/2 SSE bootstrap flush so IDEs don't freeze waiting for the first upstream byte
- Context-aware pipe teardown on client cancel (no goroutine leaks — verified with k6 + pprof)

**Tech highlights:**

- Go Phase 1 reference implementation + Rust Phase 2 port (`redb` cache, axum router)
- bbolt cache with 8-byte TTL prefix, ZSTD-compressed payloads (auto-detect via magic bytes)
- Offline mock upstream + k6/vegeta benchmarks — no API keys needed to develop
- Tag-triggered CI: 4-arch cross-compile → GitHub Releases + npm + VS Code Marketplace + Homebrew tap

**Try it (30 seconds):**

```bash
# npm
npm install -g @kortosystems/proxy-engine
kortolabs-proxy

# Homebrew (macOS)
brew tap kotro-labs/tap
brew trust kotro-labs/tap
brew install kortolabs-proxy

# Docker (mock upstream + Rust proxy)
git clone https://github.com/kotro-labs/kotro-proxy-engine.git
cd kotro-proxy-engine && docker compose up
```

Point your SDK at `http://localhost:8080/v1`.

VS Code / Cursor extension: search **KortoLabs Proxy Engine** (publisher: `kortosystems`).

**Cancel-storm audit** (the part I'm most proud of):

```bash
make cancel-audit   # 500 VUs disconnect mid-stream → 0 goroutine delta
```

Repo: https://github.com/kotro-labs/kotro-proxy-engine

Happy to answer questions on the SSE frame pipeline, cache wire format, or the Rust port strategy.

---

## Posting tips

1. Post **Tuesday–Thursday, 8–10am US Eastern** for best HN visibility.
2. Link directly to the **GitHub repo**, not a blog wrapper.
3. Be ready in comments for: comparison to LiteLLM/Portkey, why not just use Redis, AGPL concerns.
4. Reply quickly to first 10 comments — HN ranking rewards engagement in the first hour.

## Suggested follow-up subreddits (separate posts, not cross-post spam)

| Subreddit | Angle |
|-----------|-------|
| r/golang | SSE bootstrap flush, pipe watchdog, bbolt TTL sweep |
| r/rust | Phase 2 port, redb + axum pipeline, cancel-storm RSS audit |
| r/LocalLLaMA | Self-hosted token savings via semantic cache |
| r/devops | Tag → 4-arch CI → npm/Marketplace/Homebrew release engine |
