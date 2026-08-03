# Security Policy

Kotro sits inline on the traffic path between an AI coding agent and its
model provider / MCP tools, and it's designed to catch secret exfiltration
and prompt injection — which makes vulnerabilities in Kotro itself
unusually high-impact. Please report them privately.

## Reporting a vulnerability

**Do not open a public GitHub issue for a security vulnerability.**

Report privately via a [GitHub Security Advisory](https://github.com/kotro-labs/kotro-proxy-engine/security/advisories/new).
This is also the route linked from the issue template picker.

Please include:

- A description of the vulnerability and its impact (e.g., bypasses the
  injection scanner, defeats redaction, breaks tenant/scope isolation,
  escapes the MCP schema-admission gate).
- Steps to reproduce, ideally against the offline mock upstream
  (`cmd/mockupstream`) so no real API keys are needed.
- Affected version(s) / commit, and which engine (Rust `rust/kotro-proxy`
  or the frozen Go reference in `internal/`).

We'll acknowledge reports as soon as possible and aim to keep you updated
as we work toward a fix. Coordinated disclosure is preferred — please give
us a reasonable window to ship a patch before any public write-up.

## Scope

In scope:

- The Rust proxy (`rust/kotro-proxy`, `rust/kotro-core`, `rust/kotro-types`,
  `rust/kotro-schema`) — this is the actively developed engine.
- The MCP wrap path (`kotro-proxy mcp-wrap`), schema admission, and
  TaskEnvelope verification.
- Release/distribution integrity (npm, Homebrew, VS Code extension,
  install script, cosign/SBOM signing).

Out of scope / lower priority:

- `internal/` (Go, Phase 1) — frozen at `v0.1.0-go`, kept as a behavioral
  reference. Compile-only in CI. Still report issues here if you find them,
  but fixes land in Rust first.
- Findings that require the operator to have already disabled a guardrail
  (e.g. `KOTRO_MODE=disabled`) and then treat the resulting behavior as a
  bug — that's the documented, intentional effect of the dial. See
  `docs/security/THREAT-MODEL.md`.

## What we consider a real gap vs. a known, documented one

Several security gaps in Kotro today are known and intentionally
documented rather than silently missing — see the `gap_reason` field on
scenarios in `testdata/escape-lab/scenarios.json` and
`docs/security/ESCAPE-LAB-MATRIX.md` for the current, live-tested picture
of what is and isn't covered (e.g. unauthorized network egress and
cross-session filesystem persistence are open, tracked gaps, not
oversights). If you're reporting one of those, a report is still welcome,
but it'll help to check that list first so we can focus triage on genuine
surprises.

## Threat model

For the trust boundaries this policy assumes (client→proxy, proxy→
upstream, multi-tenant gateway mode, what the cache DB contains, why
`RemoteAddr` is trusted over `X-Forwarded-For`), see
[`docs/security/THREAT-MODEL.md`](docs/security/THREAT-MODEL.md).

## Supported versions

Kotro is pre-1.0 and moving quickly; only the latest release is
supported. Please reproduce against `main` or the latest tagged release
before reporting.
