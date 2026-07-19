# Show HN Draft

**Best posting times: Tuesday or Wednesday, 8–10am US Eastern.**

**Submission link:** https://github.com/kotro-labs/kotro-proxy-engine  
**(Not a blog post — link the repo.)**

---

## Title (use this)

> Show HN: Kotro – local firewall for Claude Code and Cursor that blocks MCP prompt injection

Backup if you want a softer security lead:

> Show HN: Kotro – localhost proxy that scans MCP tool results before they hit the model

---

## Body

```
I built a local proxy that sits between Cursor / Claude Code and the LLM provider.
One ~15MB Rust binary on localhost — no SaaS, nothing else in the request path.

The problem I kept hitting: poisoned MCP / tool / file text rides into the *next*
/v1/messages or /v1/chat/completions body. The model never sees a separate "MCP
channel" — it just sees more prompt. So I scan that HTTP body before it leaves.

Default: warn (x-kotro-injection-warning) + dashboard "Injections Detected".
Hard-block mode (KOTRO_INJECTION_BLOCK=true): HTTP 400 + "Blocked" count.
(Budget hard-stops are HTTP 429 — different path.)

Honest constraint: Kotro sits on the HTTP path to the provider, not raw MCP stdio.
When the agent folds a tool/file result into the next API call, the scanner sees it.

Repro (no API key — mock upstream):
  git clone https://github.com/kotro-labs/kotro-proxy-engine
  cd kotro-proxy-engine && make demo-injection
  # dashboard during the hold: http://127.0.0.1:9090/dashboard

~78s narrated demo + dashboard screenshot are in the README / docs/launch/assets/.

Secondary habit (same proxy): exact-match cache cut a real Cursor day ~68%
(make demo-savings). Also redacts secrets outbound and restores them on the stream.
Circuit breaker trips after identical tool spam; optional session token budget → 429.

Install:
  curl -sL https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/scripts/install.sh | bash
  # or: brew install kotro-labs/tap/kotro-proxy
  # or: npm i -g @kotro-labs/proxy-engine
  kotro-proxy
  # Point the agent base URL at http://127.0.0.1:8080/v1

MIT. Rust (Axum/Tokio). Go reference is frozen.

The question I actually want feedback on: is on-device MiniLM semantic cache
(~26ms/request) worth it vs exact-match SHA-256 alone? Exact-match covers most of
my hits; fuzzy matching helps agent retry paraphrases. Curious what you'd ship.
```

---

## Attachments when posting

1. Dashboard screenshot: `docs/launch/assets/dashboard-injection-demo.png`  
   (Injections Detected / Blocked, traffic table with red BLOCKED pills)
2. Optional: link the narrated MP4 from the README hero

---

## Pre-post checklist

- [x] Title is security-first (firewall / injection), savings second in the body
- [x] Status codes in copy: injection **400**, budget **429**
- [x] Honest path: HTTP body scan, **not** MCP stdio intercept
- [x] Tile labels: Detected vs Blocked (warn vs `KOTRO_INJECTION_BLOCK=true`)
- [x] Fresh-machine install: `scripts/install.sh` **and** `brew install kotro-labs/tap/kotro-proxy` → ships release **v0.5.2** (binary `--version` may print crate `1.0.0`)
- [ ] Post Tue/Wed **8–10am US Eastern**
- [ ] Submission URL = repo

---

## Notes

- The closing MiniLM question invites technical discussion — better than a pure announcement.
- Do **not** same-day crosspost to Reddit; wait ~48h (r/cursor, r/LocalLLaMA).
- Dev.to mass-market piece (cost → security reveal) is separate:  
  `devto-article-3-savings-then-security.md` — do not use that as the Show HN body.
