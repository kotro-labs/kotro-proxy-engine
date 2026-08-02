# Show HN: Kotro — local agent Flight Recorder + Kill Switch

**Title (paste into HN):**

> Show HN: Local LLM proxy with exact SSE cache, agent flight recorder, and kill switch

**URL:** https://github.com/kotro-labs/kotro-proxy-engine

---

## Post body

I built Kotro — a single-binary local reverse proxy between coding agents (Continue, Claude Code, Cline, Cursor via HTTPS bridge) and OpenAI/Anthropic-compatible providers.

**Problem:** Agent loops burn tokens silently. Exact cache hits never fire if you never see traffic. Hosted gateways add a third party in the path. After the OpenAI→Hugging Face agent incident, I wanted a **local black box** for traffic *I* control — not claims about lab sandboxes.

**What ships (honest):**

- Exact prompt-state SSE cache → `X-Kotro-Cache: HIT` on identical streams
- Circuit breaker on repeated prompt-state / identical tool args (`X-Kotro-Circuit-Open`)
- Agent Flight Recorder — append-only tape of hashes + governance events on `:9090`
- Kill switch — `POST /api/kill-switch` or rate / tool-round caps (`observe` | `enforce`)
- Injection scan + secret redaction on the provider HTTP path

Optional MiniLM paraphrase cache is **off by default**. Cursor Auto/Chat cannot hit `localhost` (cloud SSRF policy) — use Continue/Claude Code, or the HTTPS bridge guide.

**Prove it in &lt;60s (no API key):**

```bash
git clone https://github.com/kotro-labs/kotro-proxy-engine.git && cd kotro-proxy-engine
make demo-cache-hit      # MISS → HIT
make agent-guard-demo    # death loop → circuit open + flight tape
```

Dashboard: `http://127.0.0.1:9090/dashboard` (Flight Recorder + Export JSON).

Install: `brew install kotro-labs/tap/kotro-proxy` or `npm i -g @kotro-labs/proxy-engine`

Happy to dig into the SSE frame pipeline, cache key strategies, or the flight-recorder wire format.

---

## Posting tips

1. Tue–Thu, 8–10am US Eastern.
2. Link the **GitHub repo**, not a blog wrapper.
3. Lead with the demo output, not MoE/embeddings dreams.
4. Reply fast in the first hour.
