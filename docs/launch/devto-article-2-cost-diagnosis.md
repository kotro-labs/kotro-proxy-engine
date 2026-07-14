---
title: "5 Reasons Your Cursor and Claude Code Bills Are Higher Than They Should Be"
published: true
description: "Most AI coding assistant costs aren't from the prompts you type — they're from the invisible tax of agent loops, repeated prompts, and reasoning model waste. Here's how to diagnose and fix each one."
tags: ai, productivity, rust, opensource
cover_image: https://dev-to-uploads.s3.us-east-2.amazonaws.com/uploads/articles/dmmju0ydmmk5i7tei24d.jpg
---

You set up Cursor or Claude Code, started using it daily, and then saw the bill.

It was higher than you expected. Maybe a lot higher. And the frustrating part is you can't easily tell *why* — the API dashboard shows total tokens, but not what caused the spike.

Here's the thing: most of the cost isn't from the prompts you actually type. It's from invisible overhead that accumulates silently in the background. Once you know what to look for, the waste is obvious — and fixable.

These are the five root causes, roughly in order of impact.

---

## 1. Agent Retry Loops Are Burning Money While You're Not Looking

This is the biggest one.

When an agent (Cursor in Composer mode, Claude Code running a multi-step task) hits an error, it doesn't stop — it retries. If the retry produces the same error, it retries again. If nothing in the loop changes — same broken code, same error message, same prompt — it will keep sending that exact payload to the LLM until it exhausts its attempt budget or you notice.

Each iteration costs the same as the original request. A 2,000-token prompt retried 10 times costs 20,000 tokens. A complex architectural request retried 20 times in an agent loop while you step away for lunch costs real money.

**The fix:** You need something watching for this at the proxy layer. A circuit breaker that detects 3+ identical payloads in a short window and aborts — returning a signal to the agent that it's looping rather than letting it keep burning.

---

## 2. You're Paying for the Same Response Multiple Times

Think about how agents actually work in a coding session:

- You ask the same question twice (you forgot you already asked)
- The agent reads the same file in multiple turns to maintain context
- CI runs the same fixture prompts across parallel jobs
- You start a new session and the first few exchanges re-establish the same context as yesterday

Every one of these fires a fresh API call. None of them need to.

A SHA-256 exact-match cache on your local machine handles this transparently. The first time a prompt is sent, the response is cached. Every subsequent identical prompt — whether it's 5 minutes or 5 hours later — returns instantly from cache. The LLM never sees it.

In a typical day of Cursor usage, this alone cuts costs by **68%**.

---

## 3. Reasoning Models Are Running on Everything

Claude's extended thinking mode and OpenAI's o1/o3 are genuinely powerful for hard problems. They're also expensive — sometimes 10–20x more per request than a standard model call.

The issue is that most AI coding assistant tasks don't need deep reasoning. Fixing a typo doesn't need 8,000 thinking tokens. Reformatting JSON doesn't need a chain-of-thought. Adding a docstring doesn't need o3.

But if your IDE is configured to use a reasoning model by default, or if you've set a high thinking token budget "just in case," you're paying reasoning-model prices for work that a standard model handles just as well.

**The fix:** Cap `thinking.budget_tokens` (Anthropic) or `max_completion_tokens` (OpenAI) at the proxy layer. Something like 2,000 tokens covers 95% of real coding tasks. Reserve the full budget for when you explicitly need it.

---

## 4. MCP Tool Calls Are Firing Redundantly

If you're running MCP-enabled agents (Cursor with MCP tools, Claude Code with file access or web search), you're likely hitting this.

MCP tool calls fire repeatedly across turns. A file listing that hasn't changed gets re-fetched every turn. A status check that returns the same result runs 15 times in a session. A web search for the same documentation page fires twice because two agent turns need it.

Each MCP tool call result flows back through the LLM as context — you pay for those tokens both on the way in (as part of the next prompt) and as part of maintaining context across turns.

**The fix:** Cache MCP tool results with sensible TTLs. Read operations: 30 seconds. Status checks: 5 minutes. Search results: 1 hour. Write operations invalidate their scope. The agent never notices — it gets the same result, faster, at zero cost.

---

## 5. Your Context Window Is Full of Boilerplate You're Paying to Ignore

Every token sent to the LLM costs money, including tokens the model ignores.

In a typical Cursor session with MCP tools enabled:

- MCP tool schemas are included in every request — even turns where you're not calling those tools
- The same file headers and import blocks are re-sent across multiple turns
- License headers, auto-generated comments, and boilerplate take up tokens the model skims past

None of this changes between turns. All of it costs tokens.

This is harder to fix without an AST-aware context pruner, but being deliberate about which context you include and excluding static boilerplate manually can cut 20–30% from your context footprint.

---

## The Pattern Behind All Five

Look at what these have in common: **they're all overhead that happens below the prompt level**. You type a question, but behind the scenes the agent is firing retries, sending duplicate prompts, invoking the same tools, and padding the context with static content.

None of this shows up as a line item in your dashboard. It all rolls up into "tokens used."

The most practical fix is a local proxy that intercepts this traffic before it hits the cloud — catching loops, deduplicating prompts, caching tool results, and capping reasoning budgets. This runs on localhost so there's no latency added for cache misses, and zero latency for hits.

I built [Kotro](https://github.com/kotro-labs/kotro-proxy-engine) to do exactly this. It's a 15MB Rust binary that handles all five of the issues above:

- **Circuit breaker** — detects 3+ identical tool calls and trips before the loop burns more money
- **SHA-256 exact-match cache** — microsecond replay for repeated prompts (redb, no external dependencies)
- **Reasoning budget controller** — caps `thinking.budget_tokens` / `max_completion_tokens` per request
- **MCP tool response cache** — per-category TTLs (read=30s / status=5m / search=1h)
- **On-device semantic cache** — MiniLM embeddings catch rephrased variants of cached prompts

Install takes 30 seconds:

```bash
# macOS
brew install kotro-labs/tap/kotro

# Linux/macOS
curl -sL https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/scripts/install.sh | bash
```

Then point your IDE at `localhost:8080` instead of `api.openai.com` or `api.anthropic.com`.

---

## What to Check First

If your bill is high and you're not sure where to start, here's the order:

1. **Check for retry loops first** — look at your agent logs for repeated identical payloads. If you see the same error message being sent 5+ times in a session, you have a loop problem.
2. **Look at your reasoning model usage** — if you're using Claude extended thinking or o1/o3, check what percentage of requests are using it and whether simpler tasks are being routed there.
3. **Count your MCP tool calls** — if you have MCP tools enabled, check how many times the same tool is being called with the same inputs across a session.
4. **Cache hits will tell you your duplication rate** — if you add a caching proxy and see 40%+ cache hits in the first day, that's 40% of your previous spend that was duplicate work.

The numbers compound fast. A 68% reduction sounds dramatic, but it's what happens when you fix all five of these at once on a real working day.

---

GitHub: [kotro-labs/kotro-proxy-engine](https://github.com/kotro-labs/kotro-proxy-engine) — MIT license.

If you've noticed other sources of runaway cost I haven't covered here, I'd genuinely like to hear about them in the comments.
