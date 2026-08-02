# Kotro Proxy Engine — Threat Model & Security Architecture

**Version:** 0.2.0  
**Status:** Documents behavior shipped in Go Phase 1 and Rust Phase 2, plus the Local Agent Guard governance plane (flight recorder, kill switch, MCP action plane)  
**Audience:** Security reviewers, platform engineers, design partners

---

## 1. System overview

Kotro is a **local or cluster-adjacent reverse proxy** that intercepts streaming LLM traffic (`POST /v1/chat/completions`, `POST /v1/messages`), applies cache / redaction / context compression, and forwards to an upstream provider.

```
Client (IDE / SDK / agent)  →  Kotro (:8080)  →  Upstream (OpenAI / Anthropic / mock)
                                    │
                                    ├─ Semantic SSE cache (bbolt, on-disk)
                                    ├─ PII / secret redaction (regex, per-request)
                                    └─ Context compressor (in-memory, scoped LRU)
```

**Primary deployment modes:**

| Mode | Typical use | Trust model |
|------|-------------|-------------|
| **Local sidecar** (default wedge) | `localhost:8080`, VS Code extension, `brew install` | Loopback or single-user machine; credential-derived isolation |
| **Gateway / ingress** (enterprise upgrade) | K8s sidecar or shared cluster proxy | Requires explicit gateway configuration and CIDR allowlists |

---

## 2. Security objectives

| Objective | Mechanism |
|-----------|-----------|
| **Tenant isolation** | Cache keys and compressor state are scoped by `tenantID:sessionID` |
| **No credential leakage in scope IDs** | API keys are hashed (SHA-256, first 8 bytes hex) before use as scope identifiers |
| **Safe gateway defaults** | Header-based tenant assignment is **off** unless explicitly enabled |
| **Forgery resistance** | Trusted-peer checks use **TCP `RemoteAddr` only** — never `X-Forwarded-For` |
| **Resource bounds** | Request body cap, bounded compressor memory, cache TTL + eviction |
| **Upstream secret hygiene** | Redaction replaces detected secrets with placeholders before upstream call |

---

## 3. Trust boundaries

### 3.1 Boundary A — Client → Kotro

**Assumption:** In local sidecar mode, the client and proxy run on the same host or trusted network segment.

**Risks:**

- Any process that can reach `:8080` can submit requests **using credentials present in those requests**.
- Kotro does **not** authenticate clients independently; it forwards provider credentials from the request.

**Mitigations:**

- Bind to loopback in dev: `KOTRO_LISTEN_ADDR=127.0.0.1:8080`
- Use host firewall / network policy in shared environments
- Do not expose an unauthenticated proxy to the public internet
- When using a public HTTPS tunnel (Cursor Chat), set `KOTRO_BRIDGE_TOKEN` + `KOTRO_UPSTREAM_API_KEY` so URL-only callers get **401** (see `docs/guides/CURSOR-BRIDGE.md`)

### 3.2 Boundary B — Kotro → Upstream provider

**Assumption:** Kotro is a **data-plane intermediary**, not a credential vault.

**Risks:**

- Redaction is pattern-based (regex); novel secret formats may slip through
- Passthrough routes (`/v1/*` other than intercepted endpoints) forward unmodified

**Mitigations:**

- Enable redaction by default (`KOTRO_ENABLE_REDACTION=true`)
- Review `internal/guardrail/redactor.go` patterns for your org's secret formats
- Restrict passthrough surface if not required

### 3.3 Boundary C — Multi-tenant gateway (optional)

When `KOTRO_TRUST_UPSTREAM_GATEWAY=true`, Kotro accepts `X-Tenant-ID` and `X-Session-ID` **only** from peers whose **socket address** falls in `KOTRO_TRUSTED_PROXY_CIDRS`.

```
                    ┌─────────────────────┐
  Untrusted client  │  Trusted ingress /  │  Kotro
  (cannot set       │  API gateway        │  (validates
   scope headers)   │  (sets X-Tenant-ID) │   RemoteAddr ∈ CIDR)
                    └─────────────────────┘
```

**Critical invariant (shipped):** `isTrustedPeer()` inspects `r.RemoteAddr` only. HTTP forwarding headers such as `X-Forwarded-For` are **never** used for trust decisions. An untrusted client cannot spoof tenant scope by setting forwarding headers.

**Fail-safe behavior:** If `KOTRO_TRUSTED_PROXY_CIDRS` is malformed, Kotro logs an error and treats the CIDR list as **empty** (no peers trusted).

---

## 4. Tenant & session isolation

Implementation: `internal/proxy/scope.go` (Go), `rust/kotro-proxy/src/router/scope.rs` (Rust).

### 4.1 Default mode — credential-derived scope

When `KOTRO_TRUST_UPSTREAM_GATEWAY=false` (default):

1. Extract credential from `Authorization: Bearer <token>` or `x-api-key`
2. If present: `SHA-256(credential)` → first 8 bytes as hex → scope ID `cred:<hash>`
3. Both `TenantID` and `SessionID` are set to the same `cred:<hash>` value
4. If absent: fall back to `default:default` (shared scope — acceptable for anonymous local mock only)

**Properties:**

- Raw API keys never appear in cache keys, compressor maps, or logs as scope identifiers
- Different credentials → different cache and compressor partitions
- Same credential → shared cache (intended for single principal)

### 4.2 Gateway mode — header-assigned scope

When `KOTRO_TRUST_UPSTREAM_GATEWAY=true` **and** the immediate TCP peer is in `KOTRO_TRUSTED_PROXY_CIDRS`:

| Header | Required | Purpose |
|--------|----------|---------|
| `X-Tenant-ID` | Yes (else credential fallback) | Organizational tenant partition |
| `X-Session-ID` | No (defaults to credential hash or `default`) | Finer session partition within tenant |

### 4.3 Where scope is enforced

| Subsystem | Scope usage |
|-----------|-------------|
| **Semantic cache** | `KeyForRequest(..., scope.Key())` — see `internal/cache/semantic.go` |
| **Context compressor** | Per-scope LRU entry in `StateTracker` — see `internal/compressor/context.go` |
| **Redaction map** | Per-request only (not cross-request); not tenant-scoped by design |

**Test coverage:** `TestCacheIsolation_TenantSeparation`, `TestAnthropicCacheIsolation_TenantSeparation` in `internal/proxy/`.

---

## 5. Data at rest & in memory

| Store | Location | Contents | Isolation |
|-------|----------|----------|-----------|
| **Cache DB** | `KOTRO_CACHE_DB` (default `./kotro-cache.db`, bbolt) | Full captured SSE streams | Keys include scope; entries expire per `KOTRO_CACHE_TTL` |
| **Compressor state** | In-process LRU | Prior-turn content block hashes | Bounded by `KOTRO_COMPRESSOR_MAX_SCOPES`, evicted after `KOTRO_COMPRESSOR_SCOPE_TTL` |
| **Redaction map** | Per-request heap | Placeholder ↔ original secret mappings | Discarded after request completes |

**Implication for enterprise:** On shared hosts, treat the cache DB as **sensitive** — it contains full model responses. Use filesystem permissions, encrypted volumes, or per-tenant cache paths for multi-tenant hosts.

---

## 6. Denial-of-service & abuse controls

| Control | Default | Env var |
|---------|---------|---------|
| Max request body | 10 MiB | `KOTRO_MAX_REQUEST_BODY_BYTES` |
| Compressor scope cap | 10,000 entries | `KOTRO_COMPRESSOR_MAX_SCOPES` |
| Compressor idle TTL | 1 hour | `KOTRO_COMPRESSOR_SCOPE_TTL` |
| Cache entry TTL | 24 hours | `KOTRO_CACHE_TTL` |
| HTTP read timeout | 30s | `KOTRO_READ_TIMEOUT` |
| HTTP idle timeout | 120s | `KOTRO_IDLE_TIMEOUT` |

**Profiling endpoint:** `/debug/pprof` is **disabled** by default (`KOTRO_ENABLE_PPROF=false`). Enable only on trusted networks for leak audits.

---

## 7. Threat scenarios

| Threat | Likelihood (local sidecar) | Likelihood (shared gateway) | Current mitigation | Residual risk |
|--------|---------------------------|----------------------------|-------------------|---------------|
| Cross-tenant cache hit (data leak) | Low (credential-scoped) | Medium if misconfigured | Scope in cache key; tests | Shared `default:default` if no credential |
| XFF spoofing to hijack tenant scope | N/A locally | High if misimplemented | **Not used** — socket-only trust | Misconfigured reverse proxy in front of Kotro |
| Secret exfiltration via upstream | Medium | Medium | Regex redaction | Incomplete pattern coverage |
| Cache poisoning | Low | Medium | Key = prompt state + model + provider + scope | Malicious client with valid creds |
| Memory exhaustion | Low | Medium | Body limit + LRU bounds | Very large concurrent streams |
| Local port exposure | Medium | Low | Bind to loopback / firewall | Any local process can call proxy |

---

## 8. Configuration reference (security-relevant)

```bash
# Safe local sidecar defaults
KOTRO_LISTEN_ADDR=127.0.0.1:8080
KOTRO_TRUST_UPSTREAM_GATEWAY=false
KOTRO_ENABLE_REDACTION=true
KOTRO_MAX_REQUEST_BODY_BYTES=10485760
KOTRO_COMPRESSOR_MAX_SCOPES=10000
KOTRO_COMPRESSOR_SCOPE_TTL=1h
KOTRO_ENABLE_PPROF=false

# Enterprise gateway mode (only when behind a trusted ingress)
KOTRO_TRUST_UPSTREAM_GATEWAY=true
KOTRO_TRUSTED_PROXY_CIDRS=10.0.0.0/8,172.16.0.0/12
```

---

## 9. Governance control plane (Local Agent Guard)

The Rust proxy adds a local governance plane. Its security properties:

**Non-stream coverage (fixed in 0.6.x):** Circuit breaker, session-token budget, and
flight-tape miss recording previously required a streaming cache key, so
`stream: false` requests (the common agent path) skipped those controls while still
forwarding upstream. Governance keys are now minted for non-stream requests; the SSE
response cache remains stream-only so non-stream replies never replay cached SSE
frames. Redaction applies on both paths and records `redaction_count` on the tape.


### 9.1 Flight recorder (tamper-evident local tape)

- Events persist in an append-only redb store at `<KOTRO_STATE_DIR>/governance.redb`
  (default `~/.kotro`), bounded by capacity and age (`KOTRO_FLIGHT_RECORDER_MAX_AGE_SECS`,
  default 7 days).
- Every event carries `prev_hash`/`hash` forming a SHA-256 hash chain.
  `GET /api/flight-recorder/verify` re-walks the chain and reports modified or
  deleted events. The chain proves integrity *relative to the recorded tail*;
  an attacker with filesystem write access who truncates the entire store can
  still destroy history — the recorder is tamper-**evident**, not tamper-proof.
- Prompt fingerprints are HMAC-SHA256 keyed with a random per-install key
  stored inside the db. They are **not** dictionary-testable unsalted hashes;
  raw prompt text is never persisted by the recorder.

### 9.2 Control API authentication

- Mutating endpoints (`POST /api/kill-switch`, approvals, action-plane event
  ingestion) require a control token via `x-kotro-control-token` or
  `Authorization: Bearer`.
- The token is random-per-install, stored 0600 at `<state_dir>/control.token`,
  or supplied via `KOTRO_CONTROL_TOKEN`. Comparison is constant-time.
- Cross-origin browser requests are rejected: any `Origin` header must be a
  loopback origin. This blocks CSRF from web pages against the local control API.
- **Strict loopback binding.** The control/telemetry listener refuses any
  non-loopback bind and coerces it to `127.0.0.1` with a warning, so a bare
  `KOTRO_METRICS_ADDR=:9090` cannot publish the kill switch, approvals, or
  event ingestion to the LAN. `KOTRO_ALLOW_REMOTE_CONTROL=true` is the
  explicit, loudly-warned override for users fronting it with their own
  authenticated tunnel. Note this applies to the control listener only — the
  LLM proxy listener (`KOTRO_LISTEN_ADDR`) still binds `0.0.0.0` by default
  and is guarded by `KOTRO_BRIDGE_TOKEN`, not by the control token.
- Read-only telemetry (`GET /metrics`, `/api/dashboard`, `/api/flight-recorder`)
  stays unauthenticated but should be bound to loopback (`KOTRO_METRICS_ADDR`).

### 9.3 Kill switch

- Scoped: `llm` (halt upstream LLM forwards), `tools` (halt MCP tool calls via
  the action plane), or `all`. State persists across proxy restarts.
- `KOTRO_KILL_SWITCH_MODE=observe` records without blocking; `enforce` blocks.
- The kill switch stops **new** actions that flow through Kotro. It cannot stop
  already-running unmanaged local processes; that requires OS-level sandboxing.

### 9.4 Rate limiting

- `KOTRO_MAX_REQUESTS_PER_MINUTE` applies **per session** (token bucket keyed
  by the tenant/session scope), so one runaway agent cannot exhaust or mask
  another principal's budget.

### 9.5 MCP action plane (`mcp-wrap` / `protect`)

- `kotro-proxy mcp-wrap` relays MCP JSON-RPC (stdio and Streamable HTTP), pin
  tool metadata on first `tools/list` (trust-on-first-use), quarantine drift,
  validate `tools/call` arguments against the pinned schema, and enforce the
  deny-first local policy before forwarding.
- Every inbound method is subject to the multi-plane kill switch. Only an
  allowlist is relayed (`initialize` for back-compat, `server/discover`,
  `ping`, `tools/*`, `resources/*`, `prompts/*`, `completion/complete`,
  `logging/setLevel` (deprecated), `tasks/*`, `notifications/*`);
  other methods (for example `sampling/createMessage`) are denied.
  Full schema/policy enforcement applies to `tools/call`; other allowlisted
  methods are kill-switch gated but not argument-policy gated.
- Cacheable list/read results honor server-declared `ttlMs` / `cacheScope`
  (SEP-2549). `private` scope is keyed by wrap session; `public` may be shared
  within the wrap process. Cached `tools/list` bodies are still re-run through
  pin/quarantine on every hit. `list_changed` notifications invalidate the cache.
- W3C Trace Context (`traceparent` / `tracestate` / `baggage` in `params._meta`,
  SEP-414) is parsed on `tools/call` and stamped onto flight events as
  `trace_id` / `span_id`. New events use `schema_version: 1`, which covers those
  fields in the hash chain; legacy tapes deserialize as `schema_version: 0` and
  still verify with the frozen v0 material.
- Streamable HTTP mode emits `MCP-Protocol-Version`, `Mcp-Method`, and
  `Mcp-Name` on upstream POSTs (SEP-2243 client side). Server-side rejection of
  header↔body disagreement is out of scope until Kotro terminates Streamable
  HTTP as a server rather than wrapping as a client.
- First-seen tool metadata is trusted (TOFU). Review and `mcp repin` after
  installing or updating an MCP server.
- `kotro protect` / `unprotect` rewrite supported client MCP configs (with a
  backup) so traffic routes through the wrap. Consent-driven; never silent.
- Tool annotations are treated as untrusted hints; missing annotations get
  pessimistic defaults (writable / destructive / open-world).
- If the control/metrics listener is unreachable, mcp-wrap treats the kill
  switch as *not engaged* (fail-open for the remote halt signal) so a dead
  proxy does not brick every tool call. Local deny-first policy still applies.
  Operators who need hard fail-closed halt should keep the control plane up.

### 9.6 Cross-plane session graph

- LLM, MCP, and hook events share one canonical schema and are correlated by
  session id. Provenance labels (`untrusted_web`, `sensitive_read`,
  `network_egress`, `credential_input`, …) drive chain detection:
  lethal trifecta, drift-then-exec, credential egress, destructive storm.
- Critical chains auto-engage the tools kill switch when
  `KOTRO_CHAIN_AUTO_KILL` is enabled (default) and kill-switch mode is
  `enforce`. Evidence is reconstructible from
  `GET /api/session-graph?session=…` and the incident bundle export.

### 9.7 Client hooks and approvals

- `kotro-proxy hook install claude-code` registers PreToolUse / PostToolUse
  hooks that query the same policy engine. Decisions use Claude Code's
  `permissionDecision` contract (allow / deny / ask).
- Hook handlers **fail closed**: empty or invalid stdin yields
  `permissionDecision: deny` rather than silently allowing the tool.
- Short-lived approval grants (`POST /api/approvals`, `kotro-proxy approve`,
  and the VS Code "Review Tool Approvals" command) are keyed by
  server + tool + args hash (+ optional session) and expire.
  Queuing a pending approval requires the control token.

### 9.8 Policy, isolation, and telemetry

- Versioned `kotro-policy.yaml` (presets: `observe`, `developer`,
  `locked-down`) with deny-first precedence and workspace-local overrides
  that cannot relax a base deny. Every decision carries a rule id + evidence
  (`kotro-proxy policy check`).
- `kotro-proxy isolate docker` emits restrictive Docker Compose / MCP Gateway
  profiles (read-only mounts, egress allow-list, CPU/memory caps, secrets
  env-file). Kotro does not run containers.
- Optional OTel GenAI/MCP spans (`KOTRO_OTEL_ENDPOINT`) export operation,
  conversation id, tool name, and token counts. Content capture is off by
  default.

---

### 9.9 Phase 0 trust contracts

- **WASM plugins** do not receive credential headers by default
  (`Authorization`, `x-api-key`, cookies, bridge/control tokens). Opt in with
  `KOTRO_WASM_ALLOW_CREDENTIAL_HEADERS=true`. Plugin calls are budgeted by
  `KOTRO_WASM_TIMEOUT_MS` (default 500). Errors and overruns fail **closed**
  unless `KOTRO_WASM_FAIL_CLOSED=false`.
- **Event contract (`kotro.dev/v1`)**: action events carry `task_id`,
  `decision_id`, `rule_id`, and `policy_revision` (see
  `schemas/kotro/event-v1.json` and the `kotro-types` crate). Task signing
  lands in Phase 1; empty `task_id` means a legacy unscoped session.
- **Enforcement dial**: `KOTRO_ENFORCEMENT_MODE` (`audit`|`enforce`) aliases
  `KOTRO_KILL_SWITCH_MODE`. Audit records decisions without blocking;
  enforce blocks.

### 9.10 Numbat interoperability

- `POST /api/numbat/findings` (control-token authenticated) accepts Numbat NDJSON
  records. High-severity findings engage the tools kill switch; critical engages
  all. `kotro-proxy numbat ingest --file ~/.numbat/records.ndjson` posts the
  same payload. Kotro does not reimplement Numbat detection rules.

### 9.11 Loopback defaults

- LLM proxy default listen is `127.0.0.1:8080`. Non-loopback binds are coerced
  unless `KOTRO_ALLOW_REMOTE_LISTEN=true` (for authenticated tunnels).

## 10. Explicit non-goals (v0.2.x)


Kotro **does not** currently provide:

- Client authentication or API key issuance for LLM routes (bridge token aside)
- mTLS between client and proxy
- Field-level encryption of cache at rest
- SOC 2 / HIPAA compliance packaging
- Automatic PII classification beyond regex patterns
- OS-level sandboxing of agent child processes (compose with Claude Code
  sandboxing / Docker isolation instead)
- Governance of agent actions that never cross the wire (built-in IDE tools,
  direct filesystem access) except where client hook adapters are installed
- Full argument-level policy on non-`tools/call` MCP methods (resources/prompts
  are kill-switch gated and allowlisted, but not schema/policy matched)
- Hard fail-closed kill switch when the local control plane is down
  (mcp-wrap keeps local policy; remote halt is best-effort)

These are candidates for the enterprise track; see [../roadmap/90-DAY-ROADMAP.md](../roadmap/90-DAY-ROADMAP.md).

---

## 11. Security review checklist

Use this for internal design-partner or enterprise approval:

- [ ] Proxy bound to loopback or private network only
- [ ] `KOTRO_TRUST_UPSTREAM_GATEWAY` set intentionally (not accidentally `true`)
- [ ] If gateway mode: `KOTRO_TRUSTED_PROXY_CIDRS` matches **immediate** peer IPs, not client IPs
- [ ] Cache DB path has appropriate filesystem permissions
- [ ] Redaction patterns reviewed for org-specific secret formats
- [ ] `KOTRO_ENABLE_PPROF=false` in production
- [ ] Upstream URL points to intended provider (no open redirect)
- [ ] Passthrough `/v1/*` routes evaluated for necessity

---

## 12. Reporting

Report vulnerabilities via GitHub Security Advisories on [kotro-labs/kotro-proxy-engine](https://github.com/kotro-labs/kotro-proxy-engine/security/advisories/new).
