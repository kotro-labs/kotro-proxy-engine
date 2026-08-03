# Parallel workstreams — P0 launch push (Aug 2026)

> Three agents work **at the same time**. Boundaries are hard: do not edit another stream’s files unless fixing a merge conflict you caused.
> Source of truth: [`CONSOLIDATED-NEXT-STEPS.md`](./CONSOLIDATED-NEXT-STEPS.md)

---

## Ownership map

| Stream | Agent | Owns | Must not touch |
|--------|-------|------|----------------|
| **A — Storefront** | **Cursor** | README hero + honesty table + Escape Lab wording | `.github/workflows/*`, Show HN body (except link fixes), Escape Lab runner |
| **B — Trust CI** | **Codex** | CI gates, Scorecard/SLSA/audit/deny, MCP conformance scaffold | `README.md` hero/comparison sections, `docs/launch/show-hn-draft.md` |
| **C — Launch & corpus** | **Claude** | Show HN polish, comparison article draft, Escape Lab scoreboard design + good-first-issue list | `README.md` (Cursor owns), CI YAML (Codex owns) |

**Shared (read-only for all):** `docs/security/ESCAPE-LAB-MATRIX.md`, `docs/security/THREAT-MODEL.md`, `benchmarks/eval-suite/RESULTS.md`, `CONSOLIDATED-NEXT-STEPS.md` (only update your stream’s checkbox section at the bottom).

**Merge order if colliding:** Cursor README → Claude links Show HN to README → Codex CI last (no README dependency).

---

## Stream A — Cursor (storefront / P0-A)

**Goal:** Live README proves the dual-plane control-plane story before HN.

### Tasks
1. Rewrite README hero + “What it does” around:
   - MCP action plane + LLM plane
   - One `KOTRO_MODE` dial + kill switch precedence
   - Cost controls on the same path
   - Flight recorder / Escape Lab as evidence
2. Replace/extend comparison table with honest rows for **Pipelock**, **pxpipe**, LiteLLM, Portkey (narrow losses OK).
3. Capability matrix: TaskEnvelope, schema pins, exact-action approvals, mcp-wrap — visible above the fold or immediately under hero.
4. Explicit Escape Lab wording: *declared behaviour match ≠ attacks prevented*; cite EL-08/09/11 `none`.
5. Kill any remaining “HTTP instead of MCP” framing that contradicts mcp-wrap (keep dual-plane explanation).
6. Incognito sanity note in PR/commit message (ports, MIT, v0.6.2).
7. Align GitHub repo description / topics with dual-plane positioning.

### Done when
- [x] First screen answers: planes, dial, evidence, who should use Pipelock instead
- [x] No contradiction with `mcp-wrap` / vector-cache defaults
- [x] Commit on `main` titled for Stream A only (`ffc8886` + follow-ups)
- [x] Live GitHub description matches control-plane pitch

### Prompt to paste for Cursor
```text
You own Stream A only (docs/roadmap/PARALLEL-WORKSTREAMS.md).
Rewrite README for dual-plane coding-agent control plane + Pipelock/pxpipe honesty table + Escape Lab “declared ≠ prevented”.
Do not edit .github/workflows or show-hn-draft.md body.
Commit when green.
```

**Cursor implementation note (2026-08-02):** Stream A complete on `main`. Integration pass (README → competitive-honesty / scoreboard links) waits until Streams B+C land.
---

## Stream B — Codex (trust CI / P0-B)

**Goal:** Make the repo look like a serious security project under Scorecard skim — without blocking Storefront.

### Tasks
1. Plan + implement (or PR) whole-workspace `fmt`/`clippy` expansion beyond `kotro-schema`/`kotro-types` — if kotro-core blocks, document unblock path and gate what can be gated now.
2. Add CI jobs or workflow files for: `cargo audit` (or OSV), `cargo deny` (if `deny.toml` absent, add minimal), OpenSSF Scorecard, llvm-cov (informational artifact OK).
3. Add SLSA provenance step alongside existing cosign/SBOM in `release.yml` (or document exact follow-up PR if release.yml is fragile).
4. Scaffold MCP conformance: job stub + `docs/security/MCP-COMPATIBILITY.md` matrix skeleton (methods × status) — full suite green can be follow-up if suite is heavy.
5. Checklist doc for humans: protect `main`, require `test` + Escape Lab checks (cannot automate org settings from agent — write `docs/operations/BRANCH-PROTECTION.md`).
6. Do **not** rewrite README comparison/hero.

### Done when
- [x] Stream B commit adds CI trust gates and release provenance
- [x] BRANCH-PROTECTION.md + MCP-COMPATIBILITY.md skeleton landed
- [x] Full `cargo test --workspace` path green (336 passed, 1 ignored in kotro-proxy)

### Prompt to paste for Codex
```text
You own Stream B only (docs/roadmap/PARALLEL-WORKSTREAMS.md).
Expand CI trust: workspace fmt/clippy plan, cargo audit/deny, Scorecard, llvm-cov artifact, SLSA on release, MCP compatibility matrix skeleton, branch-protection operator doc.
Do not edit README.md hero/comparison or docs/launch/show-hn-draft.md.
Keep cargo test green. Commit/PR when ready.
```

---

## Stream C — Claude (launch narrative / corpus design)

**Goal:** HN-ready copy + Escape Lab scoreboard redesign + community issue backlog — content Codex/Cursor can link without rewriting.

### Tasks
1. Update `docs/launch/show-hn-draft.md`:
   - Positioning matches Codex thesis (coding-agent control plane vs Pipelock egress)
   - Link Escape Lab with “declared ≠ prevented”
   - Keep status codes, mode dial, kill switch, Cursor tunnel caveat
   - Pre-post checklist: mark storefront items as “owned by Cursor Stream A”
2. Draft `docs/launch/competitive-honesty.md` (Pipelock / pxpipe / LiteLLM / Portkey / Kotro) — long-form of README table for post-HN day 2.
3. Design Escape Lab v2 scoreboard spec in `docs/security/ESCAPE-LAB-SCOREBOARD.md`:
   - Columns: attempted / prevented / detect-only / known bypass / FP / latency / threat category
   - How CI stays a regression gate while public dashboard shows prevention rate
   - Proposed next 15 scenarios (titles only) toward ~30
4. Open (or write ready-to-paste) **8–10 GitHub issue bodies** in `docs/roadmap/GOOD-FIRST-ISSUES.md` mapped to P0-B / P1 (egress, canary, encoding DLP, etc.).
5. Do **not** implement CI YAML or rewrite README (link to Cursor’s sections).

### Done when
- [ ] show-hn-draft.md aligned with thesis
- [ ] competitive-honesty.md + ESCAPE-LAB-SCOREBOARD.md + GOOD-FIRST-ISSUES.md landed
- [ ] Commit/PR for Stream C docs only

### Prompt to paste for Claude
```text
You own Stream C only (docs/roadmap/PARALLEL-WORKSTREAMS.md).
Polish docs/launch/show-hn-draft.md to Codex positioning; write competitive-honesty.md, ESCAPE-LAB-SCOREBOARD.md, and GOOD-FIRST-ISSUES.md (8–10 issue bodies).
Do not edit README.md or .github/workflows.
Commit when ready.
```

---

## Sync protocol

1. Each stream branches preferred: `p0/stream-a-storefront`, `p0/stream-b-trust-ci`, `p0/stream-c-launch` — or sequential commits on `main` if solo machine with clear prefixes: `docs(stream-a):`, `ci(stream-b):`, `docs(stream-c):`.
2. After all three land, **Cursor** does a 15-minute integration pass: README links to competitive-honesty + scoreboard; Show HN links to README dual-plane section; CI badge row unchanged.
3. Then human posts Show HN (Stream C draft).

**Integration pass (2026-08-02):** complete — README ↔ competitive-honesty / Escape Lab scoreboard / MCP-COMPATIBILITY; Show HN ↔ README `#two-planes-one-dial` + scoreboard; CI badge row untouched.

---

## Status board

| Stream | Owner | Status |
|--------|-------|--------|
| A Storefront | Cursor | **complete** (`ffc8886` / `2d9241e`) |
| B Trust CI | Codex | **complete** (`5a5e248`) |
| C Launch & corpus | Claude | **complete** (`4fbc053`) |
| Integration | Cursor | **complete** — cross-links landed; ready to post Show HN |

Update this table when you finish your stream.
