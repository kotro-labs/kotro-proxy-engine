---
title: "I Cut My Claude Code Bills 68% and Stopped 2 Prompt Injections in One Day — With a 15MB Rust Proxy"
published: false
description: "I came for the API savings. I stayed because the same localhost proxy caught MCP prompt injection before it hit the model. Before/after numbers, a one-command repro, and three install paths."
tags: agentskills, ai, rust, agents
# cover_image: add after upload — dashboard screenshot works well:
# https://github.com/kotro-labs/kotro-proxy-engine/blob/main/docs/launch/assets/dashboard-injection-demo.png
---

I didn't install Kotro because I was worried about security.

I installed it because my Claude Code / Cursor bill kept climbing and I couldn't see *why* in the provider dashboard. Tokens went up. Output quality didn't. The usual suspects — model choice, context size — didn't explain the spike.

So I pointed the IDE at a 15MB Rust proxy on localhost and ran a normal coding day through it.

**API cost dropped ~68%.** Two secrets never left my machine. And before the day was over, the same proxy had **detected and blocked 2 prompt-injection payloads** riding in on tool results.

I came for the savings. I stayed for the security.

GitHub: [kotro-labs/kotro-proxy-engine](https://github.com/kotro-labs/kotro-proxy-engine) · MIT · single binary · no Redis / Postgres / vector DB.

---

## Before / after (same kind of day)

| | Bare IDE → provider | Through Kotro |
|--|--|--|
| **Repeat / retry prompts** | Full price every time | Exact-match cache replay (`x-kotro-cache: HIT`) |
| **Agent death loops** | Keep burning credits | Circuit breaker trips after identical tool spam |
| **Thinking / reasoning spend** | Easy to overpay on trivial tasks | Cap `thinking.budget_tokens` / `max_completion_tokens` |
| **Secrets in prompts** | Hit the cloud as typed | Redacted outbound, restored on the way back |
| **Poisoned MCP / tool text** | Becomes the model's next instruction | Warn header by default; **HTTP 400** when hard-block is on |
| **Bill (repro day)** | Baseline | **~68% lower** in the savings demo |
| **Injection events (repro day)** | Invisible | **2 detected, 2 blocked** on the operator dashboard |

Those aren't marketing placeholders — they're what the local demos print when you run them yourself (no API key required; mock upstream included).

```bash
git clone https://github.com/kotro-labs/kotro-proxy-engine.git
cd kotro-proxy-engine

# Cost story (~68% savings + secret redaction)
make demo-savings

# Security story (warn → HTTP 400 block + dashboard tiles)
make demo-injection
# Dashboard during the hold: http://127.0.0.1:9090/dashboard
```

Narrated / silent cuts of the injection run live in the repo:  
[`docs/launch/assets/exploit-demo-recording.mp4`](https://github.com/kotro-labs/kotro-proxy-engine/blob/main/docs/launch/assets/exploit-demo-recording.mp4)

---

## What was actually wasting the money

If you only read provider dashboards, waste looks like "tokens." Up close it's boring and mechanical:

1. **Retries and identical turns** — the agent asks the same thing again; you pay again.
2. **Tool-call loops** — same tool, same args, new surrounding context → no exact-match win unless something is watching the loop.
3. **Reasoning models on grunt work** — thinking budget left wide open.
4. **Static junk riding in every request** — schemas, repeated file headers, boilerplate you never typed.
5. **MCP tool results re-fetched** — same read/status/search payload across turns.

Kotro sits on `localhost:8080` between the agent and OpenAI/Anthropic (or a local upstream). Cache hits never leave your machine. Misses forward like a normal reverse proxy — then get cached for the next identical turn.

You don't rewrite your agent. You change the base URL.

---

## Wait — it also stops prompt injection

This is the part that surprised me, and the part worth sharing.

Kotro does **not** sit on raw MCP stdio. That's an important honesty line. It sits on the **HTTP path** to the LLM. When Cursor / Claude Code fold a tool or file result into the *next* `/v1/chat/completions` or `/v1/messages` body, that text is suddenly visible to a scanner.

If a "README" or tool payload contains the usual injection patterns (`ignore previous instructions`, fake system tags, exfil instructions, …), Kotro:

- **Warn mode (default):** forwards the request, sets `x-kotro-injection-warning`, increments **Injections Detected**
- **Block mode (`KOTRO_INJECTION_BLOCK=true`):** returns **HTTP 400** (not 403 — budgets use 429), increments **Blocked**

The offline repro uses a realistic OpenAI-shaped `role: "tool"` message with a dummy AWS example key (the canonical docs key — not a real credential). Phase A warns. Phase B hard-blocks. Anthropic `/v1/messages` gets the same 400 under block mode.

Dashboard after a clean run looks coherent: warm-up traffic (`miss` → `hit`), then `blocked` rows, with Detected/Blocked counters that match `requests_total`. No phantom security events.

![Kotro dashboard after injection demo](https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/docs/launch/assets/dashboard-injection-demo.png)

That's the viral contrast in one sentence: **the same sidecar that made the bill boring also made the poisoned tool result boring.**

Full framing + recording outline: [`docs/launch/exploit-demo.md`](https://github.com/kotro-labs/kotro-proxy-engine/blob/main/docs/launch/exploit-demo.md)

---

## Install (pick one of three)

### 1. Homebrew (macOS)

```bash
brew install kotro-labs/tap/kotro-proxy
kotro-proxy
```

### 2. curl (Linux / macOS, no sudo)

```bash
curl -sL https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/scripts/install.sh | bash
kotro-proxy
```

### 3. Editor marketplace (Cursor / VS Code)

Install the [Kotro Proxy Engine](https://marketplace.visualstudio.com/items?itemName=kotrolabs.kotro-proxy-engine) extension — it can start the local proxy for you.

Also available as `npm install -g @kotro-labs/proxy-engine` if that's how your machine prefers binaries.

Then:

```bash
# Point traffic at Kotro
export KOTRO_UPSTREAM_URL=https://api.anthropic.com   # or https://api.openai.com
kotro-proxy

# Operator UI
open http://127.0.0.1:9090/dashboard
```

Optional hard-block for injections once you've seen the warn path:

```bash
KOTRO_INJECTION_BLOCK=true kotro-proxy
```

---

## Point your agents at it

### Cursor

1. **Settings → Models**
2. Set **OpenAI Base URL** to `http://localhost:8080/v1`
3. Keep using your provider key as usual
4. Run **Kotro: Verify Cache** from the Command Palette if you want a two-request MISS → HIT smoke test

Opening `http://localhost:8080/v1/` in a browser is a BYPASS — that doesn't exercise the cache. Use chat / Verify Cache instead.

### Claude Code

```bash
ANTHROPIC_BASE_URL=http://localhost:8080 \
  KOTRO_UPSTREAM_URL=https://api.anthropic.com \
  kotro-proxy &

ANTHROPIC_BASE_URL=http://localhost:8080 claude
```

(Exact env names vary by Claude Code version — the important part is the Anthropic-compatible base URL pointing at Kotro, with Kotro's upstream set to the real Anthropic API.)

### Anything OpenAI-compatible

Aider, custom agents, SDKs:

```bash
OPENAI_BASE_URL=http://localhost:8080/v1 your-tool
```

---

## What you get in the box

- **SHA-256 exact-match cache** (redb) — the 68% workhorse on a real day
- **On-device MiniLM semantic cache** (optional) — paraphrases without an embedding API
- **Secret / PII redaction** — strip outbound, restore on stream
- **Agent loop circuit breaker** — stop identical tool spam
- **Reasoning budget caps** — Claude thinking / OpenAI reasoning controls
- **MCP tool-result cache** — TTL by category
- **Injection scanner** — warn or HTTP 400 block
- **Dashboard** — savings hero + Detected / Loops / Budget tiles

Idle RAM stays in the ~15MB neighborhood. Rust (Axum/Tokio). Go reference frozen; Rust is the shipping target.

---

## The shareable line

> I cut my Claude Code bills 68% with a local proxy — and the same day it stopped 2 prompt injections before they hit the model.

If you only need cheaper tokens, the savings demo is enough.  
If you've started wiring MCP tools into agents, run `make demo-injection` once and look at the dashboard.

**I came for the savings. I stayed for the security.**

---

GitHub: [kotro-labs/kotro-proxy-engine](https://github.com/kotro-labs/kotro-proxy-engine)

If you try it: what % cache hit rate do you see on a normal day, and did the injection warn header fire on anything you didn't expect?
