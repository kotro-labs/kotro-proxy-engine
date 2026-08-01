# Phase 0 — Trust Repair and Contracts

**Goal:** make every existing security claim true, and land the stable contracts that
Phase 1+ builds on. No new user-facing features until the acceptance gate passes.

**Duration:** ~2 weeks
**Shape:** 10 small PRs, each independently reviewable and mergeable.
**Non-goal:** egress enforcement, credential brokerage, task envelopes. Those are Phase 1+.

---

## Why this phase exists

Three defects were confirmed by code inspection. Each one makes a current claim untrue:

| Defect | Location | Impact |
|---|---|---|
| Proxy binds all interfaces by default | `config.rs:158` — `listen_addr: ":8080"` | Anyone on the LAN can reach the proxy, its cache, and its upstream credentials |
| Every header is handed to WASM plugins | `router/handlers.rs:92-96` | A plugin receives `Authorization` / `x-api-key` verbatim. "Plugins are sandboxed" is false today |
| WASM plugins are unbounded | `plugins/wasm.rs:46` — `Plugin::new(&manifest, [], true)` | WASI enabled, no timeout, no memory cap, no fuel. One bad plugin hangs the request path |

A security product that ships these while describing itself as a guard has a credibility
problem that no amount of later architecture fixes.

The second half of the phase lands the contracts (`kotro-types`, `KotroEvent v1`,
`Decision v1`) so that Phase 1 does not require re-plumbing every call site.

---

## PR sequence

Each PR states its own tests and compatibility constraints. PRs 0.1–0.3 are security
fixes and can land in any order. PRs 0.4–0.8 are contract work and are ordered.

---

### PR 0.1 — Default to loopback binding

**Modules:** `config.rs`, `server.rs`, `distributions/vscode-extension/package.json`

**Change**

- `listen_addr` default `":8080"` → `"127.0.0.1:8080"`
- New escape hatch `KOTRO_ALLOW_NON_LOOPBACK=true`. Without it, a non-loopback
  `KOTRO_LISTEN_ADDR` is rejected at startup with an explanatory error.
- Startup emits one line stating the bind address and whether it is loopback.
- Extension `kotrolabs.listenAddr` default changes to `127.0.0.1:8080` to match.

**Tests**

- default config binds loopback
- `KOTRO_LISTEN_ADDR=0.0.0.0:8080` without the flag → startup error, non-zero exit
- same with `KOTRO_ALLOW_NON_LOOPBACK=true` → binds, emits warning
- extension config default asserted in a unit test

**Compatibility**

Breaking for anyone intentionally exposing the proxy on a LAN. Ship in the release notes
under a `BREAKING` heading with the one-line remediation. This is the correct default for
a security product; the escape hatch keeps the door open deliberately rather than by
accident.

---

### PR 0.2 — Plugin header capability gate

**Modules:** `router/handlers.rs` (`run_wasm_plugins`), `plugins/wasm.rs`, `config.rs`

**Change**

- Introduce a default-deny header filter before constructing `WasmRequest`.
  Denied by default: `authorization`, `proxy-authorization`, `x-api-key`,
  `anthropic-api-key`, `openai-organization`, `cookie`, `set-cookie`,
  and any header whose name matches `(?i)(secret|token|key|credential)`.
- `KOTRO_PLUGIN_HEADER_ALLOWLIST` — explicit comma-separated opt-in for names that
  would otherwise be denied. Empty by default.
- When a header is withheld, record one `FlightEvent` (`plane: "ops"`) naming the plugin
  and the withheld header name. Never the value.

**Tests**

- `Authorization` present on the inbound request is absent from `WasmRequest.headers`
- allowlisting `authorization` explicitly lets it through
- a withheld header produces exactly one flight event containing the name and not the value
- pattern-matched custom header (`x-acme-secret`) is denied without configuration

**Compatibility**

Plugins that relied on reading auth headers will break. That is the point. The allowlist
gives an explicit, auditable path back.

---

### PR 0.3 — Plugin resource budget and failure mode

**Modules:** `plugins/wasm.rs`, `config.rs`

**Change**

- Build the Extism `Manifest` with an explicit timeout and memory ceiling rather than
  defaults: `KOTRO_PLUGIN_TIMEOUT_MS` (default `2000`),
  `KOTRO_PLUGIN_MAX_MEMORY_MB` (default `64`).
- `KOTRO_PLUGIN_FAIL_MODE=open|closed`, default `open`. `closed` rejects the request when
  a plugin errors or times out. Document that `open` is the compatibility default and
  `closed` is the security default we recommend.
- Reconsider `with_wasi: true`. If no shipped plugin needs WASI, default it off behind
  `KOTRO_PLUGIN_ALLOW_WASI`.
- Plugin failure records a `FlightEvent` with the plugin path and the failure class.

**Tests**

- a plugin that loops forever is killed at the timeout and does not hang the handler
- fail-open returns the unmodified body on plugin error
- fail-closed returns a `problem_response` on plugin error
- memory ceiling is present in the constructed manifest

**Compatibility**

Timeout is new; a slow-but-working plugin could now fail. 2000ms is generous for an
interceptor. Note it in release notes.

---

### PR 0.4 — `kotro-types` crate

**Modules:** new `rust/kotro-types/`, `rust/Cargo.toml` workspace members

**Change**

- New workspace member with **no dependency on `kotro-proxy`** — dependency flows one way.
- Defines, with `serde` + `schemars`:
  - `Principal { subject, issuer }`
  - `AgentIdentity { name, instance, workload_identity: Option<String> }`
  - `TaskId`, `ParentTaskId`, `DecisionId`, `PolicyRevision` (newtypes over `String`)
  - `InterventionPoint` enum
  - `DataLabel` (reuses the existing provenance vocabulary)
- `kotro-proxy` takes a dependency. No call sites change in this PR.

**Tests**

- serde round-trip for every type
- generated JSON Schema committed under `schemas/` and asserted stable in CI
- `cargo tree` assertion that `kotro-types` has no path dependency on `kotro-proxy`

**Compatibility**

Additive only. Nothing consumes it yet.

---

### PR 0.5 — `KotroEvent v1`: identity fields with a versioned hash chain

This is the subtle PR. Read the compatibility note before implementing.

**Modules:** `flight_recorder.rs`, `kotro-types`

**Change**

Add to `FlightEvent`:

```
schema_version: u16      // 0 = legacy, 1 = this schema
principal: String        // subject only; issuer in a separate field
principal_issuer: String
agent_name: String
agent_instance: String
task_id: String
parent_task: String
tool_call_id: String
trace_id: String         // W3C traceparent trace-id
span_id: String
policy_revision: String
decision_id: String
credential_id: String    // broker handle, never a credential value
destination: String      // network destination for egress events
data_class: String       // classification label
```

All default to empty. `session` and `provenance` stay — they are the existing correlation
and label fields and Phase 1 builds on them rather than replacing them.

**The hash chain constraint**

`FlightEvent::chain_material()` currently serializes a fixed 19-element JSON array.
Appending fields to that array changes the digest of every event and **breaks
verification of every tape already on disk**.

Required implementation:

```rust
fn chain_material(&self) -> Vec<u8> {
    match self.schema_version {
        0 => self.chain_material_v0(),  // byte-identical to today's 19-element array
        _ => self.chain_material_v1(),  // extended array, version-prefixed
    }
}
```

`chain_material_v0` must be a verbatim copy of the current function body. Do not
refactor it, do not "clean it up," do not reorder. It is a compatibility artifact and
should carry a comment saying so.

New events are written with `schema_version: 1`. Old events deserialize with
`schema_version: 0` via `#[serde(default)]` and still verify.

**Tests**

- **Golden tape test:** commit a fixture tape recorded before this change. Assert it
  verifies end-to-end after the change. This is the test that matters most in this PR.
- a mixed tape (v0 events followed by v1 events) verifies end-to-end
- a v1 event with a tampered identity field fails verification
- adding a field to `chain_material_v1` in future breaks a deliberate canary test,
  forcing a version bump rather than a silent chain break

**Compatibility**

Old tapes verify unchanged. New tapes are not readable by older binaries — acceptable and
one-directional. Note it in release notes.

---

### PR 0.6 — `DecisionRequest` / `Decision` v1 contract

**Modules:** `kotro-types`, `policy/mod.rs` (adapter only)

**Change**

- Define in `kotro-types`:

```rust
pub struct DecisionRequest {
    pub intervention: InterventionPoint,
    pub principal: Principal,
    pub agent: AgentIdentity,
    pub action: RequestedAction,
    pub provenance: Vec<DataLabel>,
    pub policy_revision: PolicyRevision,
}

pub struct Decision {
    pub id: DecisionId,
    pub verdict: Verdict,          // Allow | Deny | RequireApproval | Transform
    pub reason_code: String,       // stable, machine-readable
    pub explanation: String,       // human-readable
    pub obligations: Vec<Obligation>,
    pub expires_at: Option<String>,
}
```

- `policy::evaluate` is **wrapped**, not rewritten. A thin adapter maps the existing
  `policy::Decision` into the new type. The existing policy engine keeps working.
- `DecisionId` generated per evaluation (UUIDv7 for sortability).
- `reason_code` values come from a closed enum-backed string set so they are stable
  across releases.

**Tests**

- determinism: same `DecisionRequest` twice → identical `verdict` and `reason_code`
  (the `id` differs; assert on the rest)
- every `Verdict` variant round-trips through serde
- adapter preserves every existing policy outcome — table test over the current
  policy test fixtures

**Compatibility**

Additive. The existing `policy::Decision` remains until Phase 1 migrates call sites.

---

### PR 0.7 — Audit vs enforce mode

**Modules:** `config.rs`, `router/governance.rs`, `router/handlers.rs`

**Change**

- `KOTRO_MODE=disabled|audit|enforce`, default `enforce` (preserves current behavior).
- `audit` evaluates every policy and guardrail, records the verdict it *would* have
  applied with `enforced: false`, and takes no action.
- `disabled` skips evaluation entirely.
- Response header `x-kotro-mode` on every response.
- Dashboard shows the current mode prominently — an operator must never be unsure
  whether they are protected.

**Tests**

- audit mode: an injection payload records a `FlightEvent` with the deny verdict and
  `enforced: false`, and the request still returns 200
- enforce mode: same payload returns 400 and records `enforced: true`
- disabled mode: no guardrail events recorded
- `x-kotro-mode` present on hit, miss, and blocked responses

**Compatibility**

Default is `enforce`, so existing deployments are unchanged. `audit` is the new
onboarding path and should become the documented first step for new installs.

---

### PR 0.8 — Thread decision and policy IDs through every enforcement event

**Modules:** `router/handlers.rs`, `router/governance.rs`, `guardrail/*`, `mcp/*`,
`flight_recorder.rs`

**Change**

Every site that records an enforcement outcome — injection, budget, kill switch, tool
denial, tool drift, chain alert, approval — populates `decision_id` and
`policy_revision` from PR 0.6.

**Tests**

- an integration test that exercises each enforcement path and asserts
  `!decision_id.is_empty() && !policy_revision.is_empty()`
- a lint-style test that fails if a new `FlightKind` enforcement variant is added without
  an entry in that table

**Compatibility**

Additive to the v1 event schema from PR 0.5.

---

### PR 0.9 — Threat model and enforcement boundary

**Modules:** `docs/security/THREAT-MODEL.md`, `README.md`,
`distributions/vscode-extension/README.md`

**Change**

Publish a coverage matrix stating, per plane, what is **enforced**, what is
**observed only**, and what is **generated but not enforced**.

The known cases that must be stated plainly:

- `isolate/mod.rs` generates a Docker isolation profile. Kotro does not enforce it.
  Something else has to apply it.
- Enforcement covers traffic that transits the proxy. An agent that calls a binary
  directly, or reaches the network outside the proxy, is not covered in this phase.
- Injection scanning is pattern-based. It reduces risk; it is not a guarantee.
- Cursor Chat routes through Cursor's cloud, so localhost enforcement does not apply
  without a bridge.

Then audit `README.md` and the extension README against that matrix and correct any
sentence that claims more than the matrix supports.

**Tests**

CI link-check on the new doc. Otherwise reviewed, not tested.

**Compatibility**

Documentation only. This PR reduces claims; it never expands them.

---

### PR 0.10 — Security CI

**Modules:** `.github/workflows/`

**Change**

- `cargo audit` (advisory DB) and `cargo deny` (licenses, duplicate/banned crates) on
  every PR
- JSON Schema fixture comparison — a schema change without a version bump fails CI
- the golden-tape verification test from PR 0.5 runs on every PR
- `#![forbid(unsafe_code)]` asserted at crate roots where it currently holds

**Tests**

The workflow is the test. Verify it fails on a deliberately introduced vulnerable
dependency in a scratch branch before merging.

---

## Acceptance gate

Phase 0 is complete when all of the following hold. No Phase 1 work begins before then.

- [ ] Default install binds loopback only; non-loopback requires an explicit flag
- [ ] No credential-bearing header reaches a WASM plugin without an explicit allowlist entry
- [ ] A hostile plugin cannot hang, exhaust memory in, or crash the request path
- [ ] `kotro-types` exists, is depended on one-directionally, and has committed JSON Schemas
- [ ] A tape recorded before PR 0.5 still verifies after PR 0.5
- [ ] Every enforcement event carries `decision_id` and `policy_revision`
- [ ] `KOTRO_MODE` supports `disabled | audit | enforce`, and the dashboard shows which is active
- [ ] `THREAT-MODEL.md` exists and no README sentence claims more than it supports
- [ ] `cargo audit` and `cargo deny` pass in CI
- [ ] All 371 existing tests still pass

---

## What Phase 0 deliberately does not do

Stated so reviewers do not ask for it in these PRs:

- No `TaskEnvelope`. That is Phase 1.
- No egress proxy, no credential broker. Phase 4-class work, gated on the Escape Lab and
  at least one design partner.
- No watchdog. Phase 5.
- No MCP 2026-07-28 migration. It is high priority and starts immediately after this
  phase, but mixing a protocol migration into trust-repair PRs makes both harder to review.
- No policy engine rewrite. PR 0.6 wraps the existing engine; it does not replace it.

---

## Sequencing after this phase

1. **Escape Lab + public coverage matrix** — 8–10 reproducible scenarios, measured for
   prevention, detection, latency, and evidence completeness. Published with losses
   included.
2. **MCP 2026-07-28 + official conformance suite** — the migration window is open now.
3. **Numbat NDJSON ingest → Kotro response** — high-severity finding drives the existing
   kill switch. Complementary rather than competitive.
4. **`TaskEnvelope v1alpha1`** — signed task authority, expiry, parent ⊇ child capability
   intersection.
5. **Egress + credential broker** — only after 1 and at least one design partner.
