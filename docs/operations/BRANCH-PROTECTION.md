# Main branch protection

GitHub branch protection is repository state, not source code. Apply these
settings after the new workflows have completed successfully on `main`; do not
require a check name before GitHub has observed it at least once.

## Recommended ruleset

Target branch: `main`.

- Require a pull request before merging.
- Require at least one approving review; dismiss stale approvals after new
  commits.
- Require conversation resolution.
- Require branches to be up to date before merging.
- Block force pushes and deletion.
- Include administrators once the release workflow has been proven under the
  ruleset.
- Permit only the release automation identities that genuinely need bypass.

## Required checks

Start with the deterministic gates:

- `Rust tests`
- `Go compile check (reference implementation — frozen at v0.1.0-go)`
- `Validate scenario corpus`
- All six Escape Lab matrix jobs and `Merge published matrix`
- `cargo audit + deny`
- `Kotro MCP protocol tests`

Keep these informational until they have a stable history:

- `Workspace fmt + clippy debt (informational)` — pre-existing formatting and
  Clippy debt is measured but not hidden.
- `llvm-cov report (informational)` — promote after runtime and baseline are
  stable, then introduce a ratcheting threshold rather than an arbitrary target.
- `Official conformance harness availability` — this verifies the pinned alpha
  harness, not Kotro conformance.
- Cancel-storm audit — promote only after several consecutive stable runs.
- Scorecard — scheduled supply-chain signal, not a per-PR correctness gate.

## Workspace quality promotion

The required CI gate currently formats and lints `kotro-schema` and
`kotro-types`. Promote the whole workspace only after:

1. `kotro-core` either implements
   `rust/kotro-core/src/cache/semantic.rs` or removes the stale optional module
   declaration that prevents `cargo fmt --all -- --check` from resolving it.
2. `cargo fmt --all -- --check` passes without a mechanical diff.
3. `cargo clippy --workspace --all-targets -- -D warnings` passes.

Do that cleanup as a dedicated mechanical PR so security behavior changes are
not hidden inside formatting churn.

## Operator verification

After saving the ruleset:

1. Open a documentation-only pull request.
2. Confirm every required check appears and completes.
3. Confirm direct pushes and force pushes are rejected for non-bypass users.
4. Merge normally and verify release/tag automation still has the intended
   permissions.
5. Record the ruleset revision and date in the repository settings audit log.
