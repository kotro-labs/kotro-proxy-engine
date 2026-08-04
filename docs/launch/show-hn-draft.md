# Show HN Draft

**Canonical draft for the next Show HN.** Older marketing drafts under
`docs/marketing/` are historical.

**Best posting times: Tuesday or Wednesday, 8–10am US Eastern.**

**Submission link:** https://github.com/kotro-labs/kotro-proxy-engine  
**(Not a blog post — link the repo.)**

---

## Title (use this)

> Show HN: Kotro – local control plane for coding agents (MCP action governance + LLM protection + cost, one binary)

Backup if you want a shorter/security-only lead:

> Show HN: Kotro – localhost control plane that governs MCP tool calls and LLM traffic for Claude Code / Cursor

*(Dropped the "agent firewall" framing — Pipelock already owns that exact phrase and is a real, more egress/OS-sandbox-deep competitor. Kotro isn't trying to out-firewall Pipelock; it's the one binary that governs the whole coding-agent transaction — MCP admission through LLM request — with cost control and replayable evidence on the same path. See "Why not just Pipelock / pxpipe / LiteLLM?" below.)*

---

## Body

```
I built a local control plane that sits between coding agents (Claude Code,
Continue, Cline, Cursor via an HTTPS bridge) and both the model they call
and the MCP tools they use. One ~15MB Rust binary on localhost — no SaaS
required for the sidecar itself.

Two governed planes, one control dial (KOTRO_MODE=disabled|audit|enforce):

1) MCP action plane — `kotro-proxy mcp-wrap` on stdio or Streamable HTTP: pin
   tool metadata on first tools/list, quarantine rug-pulls, validate tools/call
   args against an admitted schema (bounded worker pool), deny-first local
   policy, optional signed TaskEnvelope for exact-action approvals. This
   intercepts MCP itself, not just the HTTP body the model sees afterward.

2) LLM plane — scans /v1/chat/completions and /v1/messages bodies for
   MCP-style prompt injection in tool results before they leave the machine.
   Default is warn (x-kotro-injection-warning); KOTRO_INJECTION_BLOCK=true
   returns HTTP 400. Also on this path: secret redaction, circuit breaker,
   optional session token budget (429), exact-match + semantic cache, kill
   switch that outranks the mode dial on both planes.

A flight recorder correlates both planes into one session tape, and a
CI-gated adversarial corpus (Escape Lab) is the regression gate for all of
it — 14/14 rows currently match declared behavior, but that's "declared
behavior matched," not "14/14 attacks prevented": 9/14 are prevent/
transform/detect, and three (encoded exfiltration, unauthorized egress,
cross-session filesystem persistence) are known, documented `none` rows —
see docs/security/ESCAPE-LAB-MATRIX.md (and the scoreboard design in
docs/security/ESCAPE-LAB-SCOREBOARD.md). Architecture diagram lives in the
README ("Two planes, one dial"):
https://github.com/kotro-labs/kotro-proxy-engine#two-planes-one-dial
I'd rather ship an honest gap than a green checkmark that doesn't mean what
it looks like it means.

Why not just Pipelock (agent egress firewall, OS-level sandboxing) or
pxpipe (Claude Code token compression via image context)? Different jobs.
Pipelock goes deeper on egress containment than Kotro does today — if you
need Landlock/seccomp-grade isolation right now, use it. pxpipe only does
cost. Kotro is the one binary doing MCP governance + LLM protection + cost
control on the same request path, with one dial and one evidence trail
across both planes.

Honest constraints (also in docs/security/THREAT-MODEL.md):
- Cursor Chat/Agent Override Base URL is called from Cursor's cloud, which
  blocks localhost (SSRF). Use Continue/Cline/Claude Code for direct localhost,
  or a temporary HTTPS tunnel + bridge auth.
- Under extreme schema-validation load, enforce mode fails closed on
  validation_unavailable; audit records and continues; disabled skips evaluation.
- An agent that shells out or opens a raw socket never transits the proxy —
  egress firewall is a later phase (tracked, not hidden — see Escape Lab EL-09).

Repro (no API key — mock upstream):
  git clone https://github.com/kotro-labs/kotro-proxy-engine
  cd kotro-proxy-engine && make demo-injection
  # dashboard: http://127.0.0.1:9090/dashboard
  # Escape Lab: python3 scripts/escape-lab.py --validate

Install:
  curl -sL https://raw.githubusercontent.com/kotro-labs/kotro-proxy-engine/main/scripts/install.sh | bash
  # Homebrew after the v0.6.2 tap sync: brew install kotro-labs/tap/kotro-proxy
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
3. Repo README **Two planes, one dial** section (first-screen architecture) — https://github.com/kotro-labs/kotro-proxy-engine#two-planes-one-dial
4. Escape Lab matrix (`docs/security/ESCAPE-LAB-MATRIX.md`) — if asked "does this actually work"
5. Escape Lab scoreboard design (`docs/security/ESCAPE-LAB-SCOREBOARD.md`) — if asked about prevention rate vs green CI
6. Deeper Pipelock/pxpipe/LiteLLM/Portkey comparison: `docs/launch/competitive-honesty.md`

---

## Pre-post checklist

**Storefront (Stream A — verified in integration pass 2026-08-02):**
- [x] README hero reads as coding-agent control plane, not generic "agent firewall"
- [x] README comparison table has honest Pipelock / pxpipe rows
- [x] No remaining "HTTP not MCP stdio" contradiction now that mcp-wrap ships
- [x] README links to competitive-honesty + Escape Lab scoreboard + MCP compatibility

**This draft (Stream C, this file):**
- [x] Title/body repositioned to control-plane thesis, not "agent firewall" (Pipelock already owns that phrase)
- [x] Status codes: injection **400**, budget **429**
- [x] Honest boundary: LLM HTTP path **and** MCP wrap (stdio / Streamable HTTP)
- [x] Mode dial + kill-switch precedence mentioned
- [x] Load-degradation: one sentence in post; detail in THREAT-MODEL
- [x] Escape Lab framed as "declared behavior matched" (9/14 covered, 3 known `none`), not "attacks prevented"
- [x] Direct "why not Pipelock/pxpipe" paragraph included so the obvious HN comment is pre-answered
- [x] Body aligned with README dual-plane section (integration pass)

**Launch gate (must be green before posting):**
- [x] `main` CI green after cache/TTL determinism fix (`4e075a9` / tip includes always-on required checks)
- [x] Branch protection enabled on `main` (see `docs/operations/BRANCH-PROTECTION.md`)
- [ ] Do **not** merge Dependabot PR #16 as-is (rand / ed25519-dalek break)

**Distribution (already verified 2026-08-02):**
- [x] Sync Homebrew tap to v0.6.2 and fresh-install reverify brew + curl
- [ ] Post Tue/Wed **8–10am US Eastern** — gate clear except Dependabot #16 caution
- [ ] Submission URL = repo

---

## Notes

- Do **not** same-day crosspost to Reddit; wait ~48h.
- Keep load-pool detail out of the HN body — one sentence max; full text in
  `docs/security/THREAT-MODEL.md` §9.5.
- Historical drafts: `docs/marketing/show-hn-v0.1.0.md`,
  `docs/marketing/show-hn-flight-recorder.md` — do not use as the live post.
