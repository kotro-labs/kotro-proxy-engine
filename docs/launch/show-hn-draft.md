# Show HN Draft

**Best posting times: Tuesday or Wednesday, 8–10am US Eastern.**

---

## Title (pick one, A/B test the first two)

**Option A** (specific number first):
> Show HN: Kotro – I cut my Cursor API bill by 68% with a 15MB local proxy

**Option B** (problem first):
> Show HN: Kotro – A local proxy that cuts LLM API costs and keeps secrets off the wire

**Option C** (if you have a compelling number like >50%):
> Show HN: Kotro – 68% cheaper Cursor/Claude Code sessions, no cloud account required

---

## Body

```
I built a local proxy that sits between your AI IDE (Cursor, Claude Code, Aider) and the
LLM provider. One binary, no dependencies, runs on localhost.

In a real day of coding with Cursor, it saved me 68% on API costs and redacted 2 secrets
before they reached OpenAI/Anthropic. [Screenshot of terminal demo]

What it does:
- Prompt-state cache: exact-match SHA-256 replay for repeated prompts (retries, CI fixtures,
  parallel agent runs)
- Semantic cache (Rust): on-device MiniLM embedding — paraphrased prompts also hit the cache
  without any external embedding API call
- PII/secret redaction: strips API keys, DB URLs, passwords, emails before they leave your
  machine; restores them in the streamed response
- Context deduplication: strips unchanged MCP tool schemas across turns (~30-50% of context
  window consumed by schemas before you type anything)
- Protocol translation: routes Anthropic-native clients through OpenAI-compatible backends
  and vice versa

Written in Rust (Axum/Tokio). The Go reference implementation is frozen; Rust is the
shipping target. Single binary, ~15MB, no Redis, no vector DB, no Postgres.

MIT license. GitHub: https://github.com/kotro-labs/kotro-proxy-engine

The one thing I want honest feedback on: is "on-device MiniLM semantic cache" worth
the ~26ms per-request overhead vs. just the exact-match SHA-256 cache? I've been running
both and exact-match handles most of my actual hits, but the fuzzy matching catches
rephrased questions in agent retry loops.
```

---

## Notes

- The question at the end is genuine and invites a technical discussion — HN responds
  better to posts that ask a real question than pure announcements.
- If your real number is below 30%, lead with the security/redaction angle instead of cost.
- Do NOT post until `brew install kotro-labs/tap/kotro` and the `curl` installer both
  work flawlessly end-to-end on a fresh machine — install friction kills launch momentum
  faster than anything else.
- Crosspost to r/LocalLLaMA and r/cursor 48h later (not same day — different audiences,
  different rhythm).
