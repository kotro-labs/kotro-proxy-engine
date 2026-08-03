# Kotro vs. Pipelock vs. pxpipe vs. LiteLLM vs. Portkey

**A longer, harder-to-misread version of the comparison table in the README.**
Numbers below are dated because this space moves fast — treat anything more
than a few weeks old as due for a recheck, and don't trust star counts from
SEO content-farm articles over a direct GitHub check (see the OmniRoute note).

Last checked: **2026-08-02**.

---

## The one-paragraph version

Kotro is a **local coding-agent control plane**: it governs MCP tool calls
(`mcp-wrap`) and LLM provider traffic under one `KOTRO_MODE` dial, with cost
control (cache, budget, compression) on the same request path and a
CI-gated adversarial corpus (Escape Lab) as evidence, not a claim. It is
not trying to be the deepest agent egress firewall (Pipelock is further
along there) or the broadest LLM gateway (LiteLLM and Portkey are). Its bet
is that no other project currently combines *MCP action admission + LLM
protection + cost control + one dial + replayable evidence* for the
specific coding-agent use case (Claude Code, Cursor, Continue, Cline).

---

## Pipelock

**What it is:** an open-source AI agent **egress firewall** — Apache 2.0
core, Go binary, ~20MB. Scans HTTP, WebSocket, MCP, and A2A traffic for
exfiltration, SSRF, and prompt injection.

**Where Pipelock is ahead of Kotro today, plainly:**

- **OS-level process containment** — Landlock, seccomp, network namespaces
  on Linux; `sandbox-exec` on macOS. Kotro has no equivalent; it scans
  traffic that transits the proxy, not the agent process itself.
- **SLSA build provenance** on releases, alongside signed binaries.
- **Canary tokens**, taint escalation, and behavioral baselining —
  detection layers beyond pattern matching.
- **Published compliance mappings** (OWASP MCP Top 10, OWASP Agentic Top
  10, MITRE ATLAS, EU AI Act, NIST AI RMF, HIPAA, SOC 2).
- **A public, tool-neutral attack-corpus benchmark**
  (`agent-egress-bench`, 213 cases) — anyone's proxy can be scored against
  it, not just Pipelock's.
- Traction: **~786 stars / ~90 forks / 30 releases** (2026-08-02). Started
  as a 2024 homelab project, no VC funding, listed in the CNCF Landscape
  under Security & Compliance.

**Where Kotro does something Pipelock doesn't do at all:**

- Pipelock has no caching, no token-budget control, no protocol
  translation — it is not a cost tool and doesn't claim to be.
- Pipelock's MCP coverage focuses on egress-side scanning; Kotro's
  `mcp-wrap` additionally does schema **pinning** (TOFU + drift
  quarantine) and **exact-action approvals** via a signed TaskEnvelope —
  admission control on the call itself, not just content scanning of it.
- One dial (`KOTRO_MODE`) governs both the MCP plane and the LLM plane
  together; Pipelock's mode matrix and kill switch are LLM/MCP-adjacent
  but not unified with a cost-control plane the same way.

**Who should use which:** if your primary need is hardening *any* agent's
network egress with kernel-level containment — including non-coding-agent
use cases like DeFi bots or general automation — use Pipelock, today, as
the deeper tool. If your workflow is specifically a coding agent (Claude
Code / Cursor / Continue / Cline) and you want MCP action governance, LLM
protection, and cost control under one dial without running two separate
tools, that's Kotro's lane.

---

## pxpipe

**What it is:** a local proxy that compresses Claude Code's bulky text
context into dense image pages before sending, cutting token spend.
~**6.9k stars / ~596 forks** (2026-08-02) — the largest project in this
comparison by star count.

**What pxpipe does that Kotro doesn't:** a genuinely different and more
aggressive compression technique (image-encoded context) with a larger,
more-marketed cost story.

**What Kotro does that pxpipe doesn't:** pxpipe is a cost tool only — no
MCP governance, no injection scanning, no redaction, no kill switch, no
audit evidence. It solves a narrower problem well; it is not a security
runtime and doesn't claim to be.

**Who should use which:** if token cost is your only pain point and
you're comfortable with image-based context compression, pxpipe's
approach is worth comparing on raw savings. If you also need any MCP or
LLM-traffic governance, pxpipe doesn't cover that ground at all — the two
tools aren't mutually exclusive in principle, but Kotro's cache/budget
controls address the same cost problem without the image-encoding
tradeoff.

---

## LiteLLM

**What it is:** the dominant open-source, self-hosted LLM proxy/router.
MIT license, 100+ provider routing, huge community, enterprise-proven at
scale.

**Where LiteLLM is ahead of Kotro, plainly:** provider breadth (100+ vs.
Kotro's OpenAI/Anthropic-compatible focus), ecosystem maturity, community
size, battle-tested at production scale in orgs far larger than a
coding-agent sidecar use case.

**What Kotro does that LiteLLM doesn't:** LiteLLM is not local-first by
design the way Kotro is (it's commonly run as shared infrastructure, not a
per-developer localhost sidecar), has no MCP action-plane governance, and
isn't purpose-built for the coding-agent workflow (no `KOTRO_PROFILE`
presets, no Cursor/Claude Code-specific install path).

**Who should use which:** if you're standing up shared LLM infrastructure
for an org across many providers, LiteLLM is the mature, proven choice —
Kotro is not trying to win that fight. If you want a single-developer,
local-first sidecar specifically for a coding agent's MCP + LLM traffic,
LiteLLM doesn't address the MCP half at all.

---

## Portkey

**What it is:** an LLMOps gateway — Apache 2.0 core (since March 2026)
with a managed platform around it. Real embedding-based fuzzy semantic
caching, guardrails (PII, jailbreak/prompt-injection detection), prompt
management, 250+ models.

**Where Portkey is ahead of Kotro, plainly:** broader guardrail surface,
managed-hosting option for teams that don't want to self-host, prompt
versioning, a funded team behind it.

**What Kotro does that Portkey doesn't:** no MCP action-plane governance
(schema admission, TaskEnvelope, tool-call approvals) — Portkey's
guardrails operate on the LLM-traffic side only, the same limitation as
LiteLLM. Kotro is also local-first with zero telemetry leaving the
machine by default; Portkey's model assumes some willingness to route
through or report to a managed layer for its richer features.

**Who should use which:** if you want LLMOps workflow features (prompt
versioning, broad guardrails, managed hosting) and don't need MCP
governance, Portkey is more feature-complete on that axis today. If MCP
tool-call governance is part of what you need, Portkey doesn't cover it.

---

## OmniRoute — a note on inflated claims

Several SEO/content-farm articles claim OmniRoute has "9k-20k+ GitHub
stars." Checked directly against GitHub as of 2026-08-02: **~4 stars, 0
forks** on the repo those articles link to. This is a useful reminder for
readers of *this* document too — verify star counts directly on GitHub,
not from blog posts optimized to rank for "best AI gateway 2026." OmniRoute
does ship a real built-in prompt-injection guard and broad provider
coverage; it's a legitimate project, just not the breakout hit the content
farms describe.

---

## Summary table

| Project | Stars (2026-08-02) | Core strength | What it doesn't do |
|---|---|---|---|
| **Kotro** | ~7★ / 1 fork | MCP action governance + LLM protection + cost control, one dial, one evidence trail | OS-level containment, SLSA, compliance mappings, canary tokens |
| **Pipelock** | ~786★ / 90 forks | Egress firewall, OS sandboxing, compliance mappings, public benchmark | No caching, no cost control, no MCP schema admission/TaskEnvelope |
| **pxpipe** | ~6.9k★ / 596 forks | Aggressive token compression via image context | No security scanning, no MCP governance, no audit evidence |
| **LiteLLM** | tens of thousands★ (large, mature community) | Provider breadth, enterprise scale, self-hosted routing | Not local-first by default, no MCP governance |
| **Portkey** | thousands★ (funded team, managed option) | Broad LLM guardrails, prompt management, managed hosting | No MCP governance, not local-first by default |

*(LiteLLM/Portkey star counts intentionally left approximate here rather
than stated to the digit — verify directly on GitHub before citing a
precise number publicly; Kotro/Pipelock/pxpipe/OmniRoute figures above
were checked directly against GitHub on the stated date.)*

---

## The honest gap, restated

Kotro's Escape Lab corpus — the same one linked from the README — already
documents where Kotro is behind, in the project's own words, not a
competitor's: unauthorized egress (EL-09), encoded secret exfiltration
(EL-08), and cross-session filesystem persistence (EL-11) are all known
`none` rows today, each with a stated compensating control or owning
phase rather than a silent gap. See
[`docs/security/ESCAPE-LAB-MATRIX.md`](../security/ESCAPE-LAB-MATRIX.md)
and the [scoreboard design](../security/ESCAPE-LAB-SCOREBOARD.md) for how
this is tracked going forward.
