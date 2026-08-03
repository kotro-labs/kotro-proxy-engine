# Dependency advisory exceptions

Dependency checks are blocking in `.github/workflows/rust-supply-chain.yml`.
Exceptions must be narrow, documented here, duplicated in `rust/deny.toml` and
the `cargo audit` invocation when necessary, and removed as soon as an upstream
upgrade exists.

## RUSTSEC-2026-0222 — Wasmtime 43.0.2

- **Path:** `kotro-proxy → extism 1.30.0 → wasmtime 43.0.2`
- **Reason temporarily accepted:** Extism 1.30.0 is the current release and
  pins the affected Wasmtime generation; there is no compatible patched Extism
  release to select today.
- **Exposure:** WASM plugins are optional, disabled unless the operator supplies
  plugin paths, and load local operator-configured modules. This does not make
  the vulnerable runtime safe; it narrows the default exposure.
- **Compensating controls:** credential headers are withheld by default; plugin
  calls have timeout/fail-closed controls; operators that do not require WASM
  plugins should leave `KOTRO_WASM_PLUGINS` unset.
- **Removal condition:** upgrade Extism to a release using Wasmtime `>=46.0.2`
  (or another RustSec-patched supported series), run the full Rust and plugin
  suites, then remove both ignores in the same pull request.

Review this exception weekly with the scheduled supply-chain workflow. Do not
add unrelated advisory IDs to the same exception.
