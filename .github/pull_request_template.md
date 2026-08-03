<!--
Thanks for sending a PR. Kotro is early and moving fast — see CONTRIBUTING.md
for the full guidance this checklist is drawn from. Delete any section that
genuinely doesn't apply, but please don't skip the checklist silently.
-->

## What does this change and why?

<!-- One or two sentences. Link the issue or the docs/roadmap/next-steps.md
     item this addresses, if any. -->

## Engine

- [ ] Rust (`rust/kotro-proxy` / `kotro-core` / `kotro-types` / `kotro-schema`) — primary target for new features
- [ ] Go (`internal/`) — frozen reference implementation; only bug-parity fixes expected here

## Checklist

- [ ] Ran the relevant test suite (`cargo test` for Rust, `go test ./...` for Go). If this touches `router/handlers.rs`, `guardrail/`, or `router/scope.rs`, I ran the full suite (`cargo test -p kotro-proxy`), not just the module I edited — see CONTRIBUTING.md on cross-module invariants.
- [ ] If this changes redaction patterns, I updated both `internal/guardrail/pattern.go` and `rust/kotro-proxy/src/guardrail/redactor.rs`, or noted below why only one changed.
- [ ] If this changes cache-key or scope logic, I added a test exercising the real request-handling wiring (see `router::scope::tests`), not just the unit in isolation.
- [ ] If this touches scope/isolation, redaction, or gateway-trust code, I read `docs/security/THREAT-MODEL.md` and this change doesn't cross a documented trust boundary — or I've called out below that it intentionally does.
- [ ] If this changes or adds security-relevant behavior, I ran/updated the Escape Lab corpus (`python3 scripts/escape-lab.py --validate`, and a live run if the scenario set changed).
- [ ] README / docs / code comments reflect what actually ships — no partial feature described as complete, no default misstated (see `docs/roadmap/next-steps.md` on this project's own history of overstating feature completeness).

## Anything a reviewer should look at closely?

<!-- Edge cases you're unsure about, things you deliberately didn't do, or
     tradeoffs you made. -->
