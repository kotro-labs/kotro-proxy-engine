# DEV follow-up: What a local agent black box would have shown

Draft for a short DEV Community follow-up (after Flight Recorder ships).

---

**Title:** What a local agent black box would have shown (Kotro Flight Recorder)

**Tags:** ai, rust, security, opensource

---

The Reuters write-up on an OpenAI agent that spent days probing Hugging Face before anyone noticed wasn't surprising if you run agents locally: once a loop starts, spend and tool calls escalate while you're looking at chat UI, not logs.

Kotro doesn't sandbox frontier-lab agents. It sits on **your** OpenAI/Anthropic-compatible HTTP path (Continue, Claude Code, Cline, Cursor via bridge) and keeps a **local Flight Recorder**:

- prompt hashes (not raw secrets)
- cache HIT/MISS
- circuit-breaker / tool-loop / rate-limit / kill-switch events

Reproduce without an API key:

```bash
make demo-cache-hit
make agent-guard-demo
open http://127.0.0.1:9090/dashboard
```

If your agent never hits localhost (Cursor Auto), you won't see a tape — that's an IDE routing limit, not a proxy bug. Use Continue or Claude Code for the honest path, then optionally the HTTPS bridge for Cursor Chat.

Repo: https://github.com/kotro-labs/kotro-proxy-engine
