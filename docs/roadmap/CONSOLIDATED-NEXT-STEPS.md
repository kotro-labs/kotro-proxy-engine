# Kotro — Consolidated Next Steps (Aug 2026)

> **Source synthesis:** Cursor · Claude Opus · Sol · Codex fact-check (2026-08-02).  
> **Supersedes** open sequencing in `next-steps.md` P3–P4. Historical P0–P2 checkboxes remain the archive.

---

## 1. Locked thesis (Codex refinement of Opus)

**Pipelock is a real, direct competitor** (~786★ / 90 forks / 30 releases as of 2026-08-02). “Local agent firewall” alone is **no longer differentiated**.

**Do not feature-chase Pipelock’s Landlock/seccomp stack.** Do not race LiteLLM on providers.

**Own this:**

> **Kotro is the open, local coding-agent control plane** that combines **MCP action governance**, **LLM-path protection**, **cost control**, and **replayable evidence** in one binary.

| Competitor role | Kotro role |
|-----------------|------------|
| **Pipelock** — agent egress + OS containment | **Kotro** — complete coding-agent *transaction*: MCP admission → LLM request, with cache/budget + Escape Lab evidence |
| **LiteLLM / Portkey** — org gateways | Adjacent; lose on purpose |
| **pxpipe** (~6.9k★) — image-context token compression | Different mission (cost via PNG pages); not a security/runtime peer |
| **OmniRoute** (~4★, not 9k–20k SEO claims) | Noise; do not weight SEO articles |

**Differentiators already in code (must become the README story):**

1. OpenAI ⇄ Anthropic protocol translation  
2. LLM caching + context efficiency on the same path as security  
3. MCP schema admission + TaskEnvelope + exact-action approvals  
4. Unified `KOTRO_MODE` + kill switch that outranks it  
5. Cross-plane session correlation / flight recorder  
6. Coding-agent install UX (`KOTRO_PROFILE`, brew/curl/extension)  
7. Escape Lab as a CI-gated regression corpus  

---

## 2. Fact-check: do not adopt Opus unchanged

| Opus / earlier claim | Reality (2026-08-02) | Action |
|----------------------|----------------------|--------|
| Kotro 0★ / 0 forks | **7★ / 1 fork** | Distribution gap remains; numbers were wrong |
| RESULTS.md still ~157 tests | **Already refreshed** — 336 pass / 1 ignore; Escape Lab 15/14 | Done — do not re-open |
| OmniRoute 9k–20k stars | **~4★ / 0 forks** (`BunsDev/omniroute`) | Ignore SEO star claims |
| pxpipe ~2.2k | **~6.9k★ / 596 forks** (`teamchong/pxpipe`) | Significant in *cost* lane only |
| “Public GitHub mismatch” (old install cmds, no MIT) | Live `main` @ recent commits; MIT + v0.6.2 published | Not a repo defect; search caches may lag |
| Semantic cache default contradiction | Feature table + config table both **`false`**; code default `false` | Fixed |
| VS Code lockfile 0.3.0 | `package-lock.json` already **0.6.2** | Fixed |
| No SECURITY.md | **Exists** at repo root | Done |
| No PR template | **Exists** `.github/pull_request_template.md` | Done |
| Pipelock “marketing vapor” | **Confirmed real** (Landlock, SLSA, canaries, A2A, …) | Weight heavily |

**Still real presentation gaps:**

- README still undersells advanced MCP governance (TaskEnvelope, pins, dual-plane) relative to the “regex + cache” first impression  
- Comparison table lacks **Pipelock / pxpipe** honesty rows  
- Escape Lab is easy to misread as “15/15 attacks prevented” — it is “declared behaviour matched”; **9/14 covered**, three explicit `none` (EL-08/09/11)

---

## 3. Recommendations we will *not* follow

| Bad move | Why | Instead |
|----------|-----|---------|
| Clone Pipelock sandbox in a hurry | Partial sandbox → false trust | `kotro isolate` as **enforced launcher** over Docker / Landlock runtime / Anthropic sandbox / macOS profiles — native sandbox only if integration fails |
| Compliance mapping before controls | Theater | Control → Escape Lab → evidence → then OWASP/NIST map with “partial/none” |
| Add A2A because competitors list it | Breadth trap (agentgateway’s game) | Finish MCP 2026-07-28 conformance first; A2A when a real coding-agent workflow needs it |
| Same-day HN + Reddit + Product Hunt for Trending | Looks promotional; splits maintainer attention | **Show HN → respond/fix → technical comparison → community posts → design-partner result → broader channels** |

---

## 4. Escape Lab evolution (Codex + prior reviews)

Keep honesty. Change the **scoreboard**:

| Today | Target |
|-------|--------|
| “14/14 match declared behaviour” | Attempts / prevented / detect-only / known bypass / FP / latency / coverage by threat category |
| 15 scenarios, 3 known `none` | Expand toward 50+ **with metrics**, mapped to OWASP / MCP-Security-Bench |
| CI fails on divergence from *declared* outcome | Also publish prevention rate — never equate “green CI” with “all attacks stopped” |

---

## 5. Sequenced plan (Codex-revised priorities)

### P0 — Before / at public launch

Split so we do not block HN forever, but also do not ship on a storefront that hides the product.

#### P0-A — Must land before Show HN (≤ 48–72h)

| # | Task | Status |
|---|------|--------|
| A1 | Rewrite README hero around **dual-plane control plane** (MCP admission → LLM path → evidence) | **Done** (`ffc8886`) |
| A2 | Honest comparison rows: **Pipelock** (egress/containment wins) · **pxpipe** (cost-via-images) · LiteLLM/Portkey | **Done** |
| A3 | Surface TaskEnvelope / schema pin / exact-action approvals / Escape Lab in first-screen capability matrix | **Done** |
| A4 | Clarify Escape Lab semantics in README (“declared behaviour”, not “attacks prevented”) | **Done** |
| A5 | Spot-check live GitHub in incognito (sanity only) | **Done** (raw `main` README + `gh repo edit` description) |
| A6 | Post Show HN (`docs/launch/show-hn-draft.md`); stay in-thread | **Todo** (human / after Stream C) |

Already green for launch: Homebrew v0.6.2, curl install, RESULTS.md, SECURITY.md, PR template, MIT, extension lockfile 0.6.2.

#### P0-B — Trust CI (start immediately; prefer green in week 1, not blocking if A1–A6 done)

| # | Task | Notes |
|---|------|-------|
| B1 | Protect `main`; require CI + Escape Lab | Org setting |
| B2 | Whole-workspace `fmt` + Clippy (today only `kotro-schema` / `kotro-types`) | Unblock kotro-core or scope gate carefully |
| B3 | `cargo audit` / OSV + `cargo deny` in CI | Supply chain |
| B4 | OpenSSF Scorecard workflow | Consumer trust signal |
| B5 | `cargo-llvm-cov` report (informational first) | Coverage |
| B6 | SLSA provenance beside existing cosign/SBOM | Opus/Codex agree; not #1 over B1–B3 |
| B7 | MCP conformance suite + compatibility matrix | Protocol depth > A2A |
| B8 | Public roadmap issues (`good first issue`) from this doc | Community |

---

### P1 — Security substance (weeks 1–8)

| # | Task | Maps to |
|---|------|---------|
| 1.1 | Enforced **egress allowlisting** + containment **integration** (not DIY Landlock) | EL-09 |
| 1.2 | Encoding-aware DLP / Unicode normalization | EL-08 |
| 1.3 | Canary tokens | Detection |
| 1.4 | Filesystem / memory-write governance via hooks + capabilities | EL-11 |
| 1.5 | OAuth/OIDC credential brokerage + audience-bound tokens | Identity |
| 1.6 | Policy on MCP **resources/prompts**, not only `tools/call` | Breadth on MCP plane |
| 1.7 | Expand Escape Lab with **prevention + FP + latency** metrics | Trust |
| 1.8 | EL-05 rug-pull harness in public CI | Tool integrity |

---

### P2 — Ecosystem & company adoption (after P1 controls exist)

| # | Task |
|---|------|
| 2.1 | Stable `kotro-core` + plugin / policy-pack SDKs |
| 2.2 | Signed tool manifests + registry provenance (beyond TOFU) |
| 2.3 | Reusable community attack corpus |
| 2.4 | Optional local-data-plane / team-control-plane |
| 2.5 | Design partners + independently measured case studies |
| 2.6 | Compliance mappings **backed by generated Escape Lab evidence** |
| 2.7 | A2A only when a real coding-agent workflow requires it |

---

## 6. Launch sequence (distribution)

1. **Show HN** (security-first draft)  
2. Rapid reply + onboarding fixes  
3. Technical comparison + Escape Lab / RESULTS deep-dive  
4. Tailored posts (Rust / MCP / security communities) — not same-day spray  
5. Design-partner writeup  
6. Broader channels (Product Hunt / Reddit) later  

---

## 7. Metrics that matter

| Leading metric | 90-day target |
|----------------|---------------|
| Install → first protected tool/LLM call | &lt; 5 minutes |
| Escape Lab: prevented / detect / bypass / FP published | Honest dashboard, ≥ 30 scenarios |
| OpenSSF Scorecard critical checks | ≥ 7/10 green |
| Design partners with numbers | ≥ 2 |
| External meaningful PRs | ≥ 3 |
| Issue response (security/install) | &lt; 48h |
| Stars | Outcome — not weekly OKR |

---

## 8. This week’s checklist

```text
P0-A (before HN)
[ ] A1 Dual-plane README rewrite
[ ] A2 Pipelock / pxpipe / LiteLLM honesty table
[ ] A3 Capability matrix surfaces MCP governance + Escape Lab
[ ] A4 Escape Lab “declared ≠ prevented” wording
[ ] A5 Incognito GitHub spot-check
[ ] A6 Post Show HN

P0-B (start in parallel / week 1)
[ ] B1 Protect main + required checks
[ ] B2 Workspace fmt/clippy plan
[ ] B3–B6 audit/deny/Scorecard/SLSA
[ ] B7 MCP conformance CI + matrix
[ ] B8 Open good-first-issue backlog
```

---

## Related docs

- Show HN: `docs/launch/show-hn-draft.md`  
- Threat model: `docs/security/THREAT-MODEL.md`  
- Escape Lab: `docs/security/ESCAPE-LAB-MATRIX.md`  
- Results: `benchmarks/eval-suite/RESULTS.md`  
- July review (historical): `docs/review/2026-07-strategic-review.md`  
- Archive checklist: `docs/roadmap/next-steps.md`  
