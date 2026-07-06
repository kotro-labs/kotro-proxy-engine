# Kotro Proxy Engine — Threat Model & Security Architecture

**Version:** 0.1.2  
**Status:** Documents behavior shipped in Go Phase 1 and Rust Phase 2  
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

- Bind to loopback in dev: `KORTO_LISTEN_ADDR=127.0.0.1:8080`
- Use host firewall / network policy in shared environments
- Do not expose an unauthenticated proxy to the public internet

### 3.2 Boundary B — Kotro → Upstream provider

**Assumption:** Kotro is a **data-plane intermediary**, not a credential vault.

**Risks:**

- Redaction is pattern-based (regex); novel secret formats may slip through
- Passthrough routes (`/v1/*` other than intercepted endpoints) forward unmodified

**Mitigations:**

- Enable redaction by default (`KORTO_ENABLE_REDACTION=true`)
- Review `internal/guardrail/redactor.go` patterns for your org's secret formats
- Restrict passthrough surface if not required

### 3.3 Boundary C — Multi-tenant gateway (optional)

When `KORTO_TRUST_UPSTREAM_GATEWAY=true`, Kotro accepts `X-Tenant-ID` and `X-Session-ID` **only** from peers whose **socket address** falls in `KORTO_TRUSTED_PROXY_CIDRS`.

```
                    ┌─────────────────────┐
  Untrusted client  │  Trusted ingress /  │  Kotro
  (cannot set       │  API gateway        │  (validates
   scope headers)   │  (sets X-Tenant-ID) │   RemoteAddr ∈ CIDR)
                    └─────────────────────┘
```

**Critical invariant (shipped):** `isTrustedPeer()` inspects `r.RemoteAddr` only. HTTP forwarding headers such as `X-Forwarded-For` are **never** used for trust decisions. An untrusted client cannot spoof tenant scope by setting forwarding headers.

**Fail-safe behavior:** If `KORTO_TRUSTED_PROXY_CIDRS` is malformed, Kotro logs an error and treats the CIDR list as **empty** (no peers trusted).

---

## 4. Tenant & session isolation

Implementation: `internal/proxy/scope.go` (Go), `rust/korto-proxy/src/router/scope.rs` (Rust).

### 4.1 Default mode — credential-derived scope

When `KORTO_TRUST_UPSTREAM_GATEWAY=false` (default):

1. Extract credential from `Authorization: Bearer <token>` or `x-api-key`
2. If present: `SHA-256(credential)` → first 8 bytes as hex → scope ID `cred:<hash>`
3. Both `TenantID` and `SessionID` are set to the same `cred:<hash>` value
4. If absent: fall back to `default:default` (shared scope — acceptable for anonymous local mock only)

**Properties:**

- Raw API keys never appear in cache keys, compressor maps, or logs as scope identifiers
- Different credentials → different cache and compressor partitions
- Same credential → shared cache (intended for single principal)

### 4.2 Gateway mode — header-assigned scope

When `KORTO_TRUST_UPSTREAM_GATEWAY=true` **and** the immediate TCP peer is in `KORTO_TRUSTED_PROXY_CIDRS`:

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
| **Cache DB** | `KORTO_CACHE_DB` (default `./kortolabs-cache.db`, bbolt) | Full captured SSE streams | Keys include scope; entries expire per `KORTO_CACHE_TTL` |
| **Compressor state** | In-process LRU | Prior-turn content block hashes | Bounded by `KORTO_COMPRESSOR_MAX_SCOPES`, evicted after `KORTO_COMPRESSOR_SCOPE_TTL` |
| **Redaction map** | Per-request heap | Placeholder ↔ original secret mappings | Discarded after request completes |

**Implication for enterprise:** On shared hosts, treat the cache DB as **sensitive** — it contains full model responses. Use filesystem permissions, encrypted volumes, or per-tenant cache paths for multi-tenant hosts.

---

## 6. Denial-of-service & abuse controls

| Control | Default | Env var |
|---------|---------|---------|
| Max request body | 10 MiB | `KORTO_MAX_REQUEST_BODY_BYTES` |
| Compressor scope cap | 10,000 entries | `KORTO_COMPRESSOR_MAX_SCOPES` |
| Compressor idle TTL | 1 hour | `KORTO_COMPRESSOR_SCOPE_TTL` |
| Cache entry TTL | 24 hours | `KORTO_CACHE_TTL` |
| HTTP read timeout | 30s | `KORTO_READ_TIMEOUT` |
| HTTP idle timeout | 120s | `KORTO_IDLE_TIMEOUT` |

**Profiling endpoint:** `/debug/pprof` is **disabled** by default (`KORTO_ENABLE_PPROF=false`). Enable only on trusted networks for leak audits.

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
KORTO_LISTEN_ADDR=127.0.0.1:8080
KORTO_TRUST_UPSTREAM_GATEWAY=false
KORTO_ENABLE_REDACTION=true
KORTO_MAX_REQUEST_BODY_BYTES=10485760
KORTO_COMPRESSOR_MAX_SCOPES=10000
KORTO_COMPRESSOR_SCOPE_TTL=1h
KORTO_ENABLE_PPROF=false

# Enterprise gateway mode (only when behind a trusted ingress)
KORTO_TRUST_UPSTREAM_GATEWAY=true
KORTO_TRUSTED_PROXY_CIDRS=10.0.0.0/8,172.16.0.0/12
```

---

## 9. Explicit non-goals (v0.1.x)

Kotro **does not** currently provide:

- Client authentication or API key issuance
- mTLS between client and proxy
- Audit log export with tamper evidence
- Field-level encryption of cache at rest
- SOC 2 / HIPAA compliance packaging
- Automatic PII classification beyond regex patterns

These are candidates for the enterprise track; see [../roadmap/90-DAY-ROADMAP.md](../roadmap/90-DAY-ROADMAP.md).

---

## 10. Security review checklist

Use this for internal design-partner or enterprise approval:

- [ ] Proxy bound to loopback or private network only
- [ ] `KORTO_TRUST_UPSTREAM_GATEWAY` set intentionally (not accidentally `true`)
- [ ] If gateway mode: `KORTO_TRUSTED_PROXY_CIDRS` matches **immediate** peer IPs, not client IPs
- [ ] Cache DB path has appropriate filesystem permissions
- [ ] Redaction patterns reviewed for org-specific secret formats
- [ ] `KORTO_ENABLE_PPROF=false` in production
- [ ] Upstream URL points to intended provider (no open redirect)
- [ ] Passthrough `/v1/*` routes evaluated for necessity

---

## 11. Reporting

Report vulnerabilities via GitHub Security Advisories on [kotro-labs/kotro-proxy-engine](https://github.com/kotro-labs/kotro-proxy-engine/security/advisories/new).
