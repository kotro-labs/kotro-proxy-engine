# Good first issues — ready to paste

Source: `CONSOLIDATED-NEXT-STEPS.md` P1 (security substance, weeks 1–8).
P0-B (CI trust: fmt/clippy, audit/deny, Scorecard, SLSA, MCP conformance
scaffold, branch protection) is Stream B's active work right now — not
duplicated here to avoid two people/agents landing the same PR. Everything
below is P1, plus one bonus item tying into the Escape Lab scoreboard
design (`docs/security/ESCAPE-LAB-SCOREBOARD.md`).

Each entry below is meant to be pasted directly as a GitHub issue body.
Difficulty is stated honestly — not everything here is a five-minute
change, and a couple are explicitly scoped down to "design/first-slice PR"
rather than the full feature, because the full feature isn't a reasonable
first PR for someone new to the codebase.

---

## 1. Egress allowlisting MVP (closes Escape Lab EL-09)

**Labels:** `good first issue` (design slice) / `help wanted` (full feature)
**Maps to:** P1.1

Kotro currently enforces only on traffic that transits the proxy — an
agent invoking `curl` directly or opening a raw socket is entirely
uncovered (see `docs/security/ESCAPE-LAB-MATRIX.md`, EL-09, and the
`gap_reason` in `testdata/escape-lab/scenarios.json`).

**First PR (right-sized for a first contribution):** a design doc under
`docs/security/` proposing the allowlist mechanism — host/IP/port
allowlist shape, where it plugs into the request path, and explicitly
whether Kotro implements egress control itself vs. composes with an
existing sandbox (Docker network policy, macOS `pf`, Linux namespaces —
see `CONSOLIDATED-NEXT-STEPS.md` §3 on why we're not cloning Pipelock's
Landlock stack from scratch).

**Full feature (follow-up PR):** implement the allowlist + a new Escape
Lab scenario (EL-17 in `docs/security/ESCAPE-LAB-SCOREBOARD.md`'s
proposed set) that flips EL-09 from `none` to `prevent`.

---

## 2. Encoding-aware secret detection (closes Escape Lab EL-08)

**Labels:** `good first issue`
**Maps to:** P1.2

`rust/kotro-proxy/src/guardrail/redactor.rs` does literal-pattern
matching only — a base64-encoded API key or connection string sails
through untouched (EL-08, `testdata/escape-lab/scenarios.json`).

**Scope for a first PR:** add a decode-then-scan pass for base64 and
URL-encoding specifically (not a general steganography detector) ahead of
the existing pattern match, with tests covering: a base64-encoded AWS key
is caught, a base64-encoded *non-secret* string is not a false positive,
and nested/double-encoding is out of scope for v1 (documented as a known
limit, not silently unhandled). Add the corresponding Escape Lab scenario
(EL-16 in the scoreboard doc) once it passes.

---

## 3. Canary tokens

**Labels:** `good first issue`
**Maps to:** P1.3

Generate a synthetic secret (e.g. a fake API-key-shaped string) that's
injected into the agent's environment on request, then watch outbound
traffic for it. If it appears, that's proof of exfiltration independent
of whether the redaction patterns caught the "real" secret.

**Scope for a first PR:** a `kotro-proxy canary generate` (or equivalent)
subcommand that emits a config snippet, plus wiring the redactor/injection
scanner to flag a canary match with a distinct evidence kind (not lumped
in with normal redaction hits, so a canary trip is unambiguous in the
flight recorder). Pipelock's `pipelock canary` is worth reading for prior
art on the UX, not the implementation.

---

## 4. Filesystem / memory-write governance (closes Escape Lab EL-11)

**Labels:** `help wanted` (design first)
**Maps to:** P1.4

Kotro has no notion of which filesystem paths carry cross-session
instructions an agent might read back later (EL-11 — cross-session
persistence via memory file write).

**Scope for a first PR:** extend `TaskEnvelope` with a filesystem
capability set (which paths a given task is allowed to write, in the
spirit of the existing schema-admission work in `rust/kotro-schema`), as
a design + schema PR before any enforcement lands. Enforcement (a
follow-up) likely needs a hook into wherever the agent's tool calls
actually touch disk, which varies by client — start with the design.

---

## 5. OAuth/OIDC credential brokerage — design doc only

**Labels:** `help wanted`, `design`
**Maps to:** P1.5

The MCP authorization spec expects OAuth-based authorization,
audience-bound tokens, and explicitly prohibits token passthrough
(an MCP server should never receive the agent's original bearer token).
Kotro currently documents no client authentication or credential
brokerage on the LLM route.

**This is not a first-PR-sized feature.** The ask here is specifically a
design doc: PKCE flow shape, where short-lived credential issuance would
live, how per-tool scopes map onto Kotro's existing scope/tenant model
(`rust/kotro-proxy/src/router/scope.rs`), and an explicit statement of
what "Kotro never forwards the agent's original token to an MCP server"
requires architecturally. Implementation is a separate, later issue once
the design is reviewed.

---

## 6. Policy on MCP `resources/*` and `prompts/*`, not just `tools/call`

**Labels:** `good first issue`
**Maps to:** P1.6

`mcp-wrap`'s schema admission and drift quarantine currently focus on
`tools/call`. `resources/read` and `prompts/get` are separate MCP
surfaces that can carry poisoned content or template injection the same
way a tool result can, and today they pass through unexamined.

**Scope for a first PR:** extend the existing injection scanner (already
used on tool-result content) to also run on `resources/read` responses
and `prompts/get` template output — reusing the scanner, not building a
new one. Add EL-21 and EL-22 (`docs/security/ESCAPE-LAB-SCOREBOARD.md`)
once wired up.

---

## 7. EL-05 rug-pull harness in public CI

**Labels:** `good first issue`
**Maps to:** P1.8

EL-05 (MCP tool rug-pull — schema drift after approval) exists in the
corpus but is `cli`-harness-only, meaning it's not exercised by the
public `escape-lab.yml` CI matrix the way the 14 HTTP-measurable
scenarios are.

**Scope:** wire an `mcp-wrap` CLI invocation into the Escape Lab CI job
(or a sibling job) that actually drives the rug-pull scenario end-to-end
against a live `mcp-wrap` process, the way `scripts/run-escape-lab-matrix.sh`
does for the HTTP scenarios. This turns EL-05 from "not measured by this
harness" into a real, public, CI-gated row.

---

## 8. Implement the Escape Lab scoreboard renderer

**Labels:** `good first issue`
**Maps to:** P1.7, `docs/security/ESCAPE-LAB-SCOREBOARD.md`

The scoreboard column schema and rollup math (prevented / detect-only /
known-bypass / FP / latency, per-category and aggregate) are already
designed in `docs/security/ESCAPE-LAB-SCOREBOARD.md` — this issue is
"build the thing the design doc describes."

**Scope:** add a `--scoreboard` render mode to `scripts/escape-lab.py`
alongside the existing `render_markdown()`, producing the table shape in
the design doc from live run output (or `--merge`d output). Keep the
existing declared-vs-observed table and CI-gate behavior completely
unchanged — this is a new, additive rendering path, not a replacement.

---

## 9. Benign-traffic (false-positive) control scenarios

**Labels:** `good first issue`
**Maps to:** `docs/security/ESCAPE-LAB-SCOREBOARD.md` "FP measurement gap"

Every scenario in the corpus today is an attack scenario — none test
whether Kotro over-triggers on legitimate traffic (e.g. a `git diff` with
a variable literally named `password`, or a code review comment
containing the word "ignore previous instructions" as a quoted example,
not an actual injection attempt).

**Scope:** add one benign-traffic counterpart scenario per existing
category (8 scenarios: injection, secret-exfiltration, resource-abuse,
tool-integrity, operator-control, egress, monitoring-integrity,
persistence) that should score as pass-through / no-detection, and wire
them so a detection on one of these counts as a false positive in the
scoreboard from issue #8. Good first issue for someone who wants to
understand the corpus format without touching Rust at all — the scenario
files are JSON.

---

## Notes for whoever triages these

- Every issue above should link back to the specific `gap_reason` /
  design doc it closes, not just restate the roadmap line — that's what
  makes these "ready to paste," not just a to-do list.
- If Stream B's CI work (`docs/roadmap/PARALLEL-WORKSTREAMS.md`) lands a
  branch-protection requirement before these are opened, add "requires
  passing CI + Escape Lab" to each issue's acceptance criteria explicitly.
- None of these should be opened as literally "good first issue" labeled
  if they're actually `help wanted`/design-scoped (#4, #5) — mislabeling
  difficulty is worse for community trust than not labeling at all.
