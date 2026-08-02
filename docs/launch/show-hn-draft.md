# Show HN Draft

**Canonical draft for the next Show HN.** Older marketing drafts under
`docs/marketing/` are historical.

**Best posting times: Tuesday or Wednesday, 8–10am US Eastern.**

**Submission link:** https://github.com/kotro-labs/kotro-proxy-engine  
**(Not a blog post — link the repo.)**

---

## Title (use this)

> Show HN: Kotro – local agent firewall for Claude Code / Cursor (LLM proxy + MCP wrap)

Backup if you want a softer security lead:

> Show HN: Kotro – localhost proxy that scans tool results and wraps MCP before they hit the model

---

## Body

```
I built a local-first agent firewall that sits between coding agents
(Claude Code, Continue, Cline, Cursor via an HTTPS bridge) and the model /
MCP tools they call. One ~15MB Rust binary on localhost — no SaaS required
for the sidecar itself.

Two enforcement planes, one control dial (KOTRO_MODE=disabled|audit|enforce):

1) LLM proxy — scans /v1/chat/completions and /v1/messages bodies for MCP-style
   prompt injection in tool results before they leave the machine. Default is
   warn (x-kotro-injection-warning); KOTRO_INJECTION_BLOCK=true returns HTTP 400.
   Also: secret redaction, circuit breaker, optional session token budget (429),
   exact-match cache, kill switch that outranks the mode dial.

2) MCP wrap — `kotro-proxy mcp-wrap` on stdio or Streamable HTTP: pin tool
   metadata on first tools/list, quarantine rug-pulls, validate tools/call
   args against an admitted schema (bounded worker pool), deny-first local
   policy, optional signed TaskEnvelope. This *does* intercept MCP, not just
   the HTTP body the model sees afterward.

Honest constraints (also in docs/security/THREAT-MODEL.md):
- Cursor Chat/Agent Override Base URL is called from Cursor's cloud, which
  blocks localhost (SSRF). Use Continue/Cline/Claude Code for direct localhost,
  or a temporary HTTPS tunnel + bridge auth.
- Under extreme schema-validation load, enforce mode fails closed on
  validation_unavailable; audit records and continues; disabled skips evaluation.
- An agent that shells out or opens a raw socket never transits the proxy —
  egress firewall is a later phase.

Repro (no API key — mock upstream):
  git clone https://github.com/kotro-labs/kotro-proxy-engine
  cd kotro-proxy-engine && make demo-injection
  # dashboard: http://127.0.0.1:9090/dashboard
  # Escape Lab: python3 scripts/escape-lab.py --validate

Install:
  curl -sL https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/scripts/install.sh | bash
  # or: brew install kotro-labs/tap/kotro-proxy
  kotro-proxy
  # Point local agents at http://127.0.0.1:8080/v1
  # Wrap an MCP server: kotro-proxy mcp-wrap --name files -- npx -y @modelcontextprotocol/server-filesystem /tmp

MIT. Feedback I'm after: is the audit→enforce rollout dial the right onboarding
path, or would you rather start enforce-by-default with a loud kill switch?
```

---

## Attachments when posting

1. Dashboard screenshot: `docs/launch/assets/dashboard-injection-demo.png`
2. Optional: narrated MP4 from the README hero
3. Optional: Escape Lab matrix link (`docs/security/ESCAPE-LAB-MATRIX.md`)

---

## Pre-post checklist

- [x] Title is security-first (firewall / injection / MCP), savings second
- [x] Status codes: injection **400**, budget **429**
- [x] Honest boundary: LLM HTTP path **and** MCP wrap (stdio / Streamable HTTP)
- [x] Mode dial + kill-switch precedence mentioned
- [x] Load-degradation: one sentence in post; detail in THREAT-MODEL
- [x] Fresh-machine install paths current
- [ ] Post Tue/Wed **8–10am US Eastern**
- [ ] Submission URL = repo

---

## Notes

- Do **not** same-day crosspost to Reddit; wait ~48h.
- Keep load-pool detail out of the HN body — one sentence max; full text in
  `docs/security/THREAT-MODEL.md` §9.5.
- Historical drafts: `docs/marketing/show-hn-v0.1.0.md`,
  `docs/marketing/show-hn-flight-recorder.md` — do not use as the live post.
