# MCP compatibility and conformance

**Status:** implementation matrix, not a claim of full MCP certification.
**Target:** MCP `2026-07-28`, with intentional compatibility for selected
2025-era lifecycle behavior.

Kotro is an inline MCP wrapper: it relays an existing client to an existing
server while applying admission, policy, cache, and evidence controls. It is
not itself a general-purpose MCP SDK, standalone client, or tool server. The
official conformance framework currently tests standalone clients and servers,
so the CI scaffold validates Kotro's protocol-specific units and pins the
upstream harness while a wrapper adapter is designed.

## Method matrix

| Surface | 2026-07-28 expectation | Kotro status | Evidence / remaining work |
|---------|-------------------------|--------------|---------------------------|
| `server/discover` | Modern capability discovery | **Relayed** | Allowlisted in `mcp::wrap`; add end-to-end modern lifecycle fixture |
| `initialize` | Removed from modern stateless lifecycle | **Legacy relay** | Retained only for compatibility; version negotiation matrix needed |
| `tools/list` | Cache hints and change handling | **Implemented** | TTL/scope parsing, invalidation, and repin-on-cache-hit tests |
| `tools/call` | Routed action with JSON Schema 2020-12 input | **Implemented** | Header/body agreement, bounded schema admission, policy, TaskEnvelope |
| `resources/list`, `resources/read` | Stateless resource operations | **Relay + cache** | Kill-switch and method allowlist; argument policy is not yet complete |
| `prompts/list`, `prompts/get` | Stateless prompt operations | **Relay + cache** | Kill-switch and method allowlist; argument policy is not yet complete |
| `completion/complete` | Completion utility | **Relayed** | Protocol fixture pending |
| `tasks/*` | Tasks extension | **Relayed** | Extension negotiation and lifecycle conformance pending |
| Multi-round-trip input | `input_required` + retry state | **Not verified** | Adapter and scenario fixtures pending |
| `subscriptions/listen` | Unified change stream | **Not verified** | 2026 lifecycle support pending |
| `sampling/createMessage` | Deprecated/removed modern path | **Denied** | Negative test required |
| `logging/setLevel` | Deprecated/removed modern path | **Legacy relay** | Remove from modern-version allowlist after negotiation exists |

## Transport and metadata matrix

| Requirement | Status | Evidence / gap |
|-------------|--------|----------------|
| stdio wrapping | **Implemented** | `mcp::wrap` unit/integration tests |
| Streamable HTTP wrapping | **Implemented** | Upstream relay tests; full official adapter pending |
| `MCP-Protocol-Version` | **Implemented** | `mcp::routing` emits `2026-07-28` |
| `Mcp-Method` / `Mcp-Name` | **Implemented** | Header derivation and header/body agreement tests |
| Per-request client metadata | **Partial** | Header path exists; complete `_meta` lifecycle matrix pending |
| W3C Trace Context in `_meta` | **Implemented** | Trace parsing and flight-event correlation tests |
| `ttlMs` / `cacheScope` | **Implemented** | Public/private cache isolation tests |
| DNS rebinding / Origin validation | **Not a server claim** | Kotro wrapper client path only; any future listener must test this explicitly |

## CI policy

`.github/workflows/mcp-compatibility.yml` currently:

1. Runs Kotro routing, cache, and wrapper tests.
2. Pins and lists scenarios from
   `@modelcontextprotocol/conformance@0.2.0-alpha.10` so upstream availability
   and scenario churn are visible.
3. Does **not** label Kotro conformant merely because those jobs are green.

Promotion to a required “MCP conformance” check requires a wrapper adapter that
executes the relevant official client/server scenarios through Kotro, an
expected-failures file reviewed into this document, and zero undeclared
failures for both the modern and legacy compatibility profiles.
