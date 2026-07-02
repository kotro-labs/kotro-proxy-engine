# Kotro 90-Day Roadmap

**Strategy:** 70% IDE / developer sidecar adoption · 30% platform / K8s ingress primitives  
**Version baseline:** v0.1.2  
**Goal:** Move from high-performance local tool → operator-grade appliance with reproducible ROI proof

---

## Strategic thesis

| Motion | Role | Why |
|--------|------|-----|
| **Developer wedge** (primary) | Top-of-funnel adoption | `brew install` → token savings in 30 seconds; no cluster change control |
| **Enterprise upgrade path** (secondary) | Expansion revenue + platform teams | Hardened scope isolation + gateway mode already shipped; needs observability + reference arch |
| **Trust artifacts** (enabler) | Unblocks both | Security doc, eval suite, SLO baselines answer "will it break?" and "what's the ROI?" |

**North-star metric:** 3 design-partner teams running Kotro on real agent workloads with published token/latency numbers.

---

## Phase map

```
Days 1–30   Trust & proof        Security doc · eval fixtures · compatibility matrix
Days 31–60  Observability        Prometheus · local dashboard · SLO baselines
Days 61–90  Adoption & GTM       Design partners · Show HN · Helm / ingress blueprint
```

---

## Weeks 1–4 — Trust artifacts & eval suite

**Primary (70%):** Coding-agent workload fixtures  
**Secondary (30%):** Local vs gateway threat-boundary documentation

| Week | Deliverable | Owner focus |
|------|-------------|-------------|
| **W1** | [THREAT-MODEL.md](../security/THREAT-MODEL.md) finalized; link from README | Security / compliance reviewers |
| **W1** | Eval fixture catalog: repetitive context reload, tool-output dumps, MCP schema blocks | IDE agent sessions |
| **W2** | `benchmarks/eval-suite/` harness: compressor on/off, cache hit/miss, parity check | Token + latency baselines |
| **W2** | Document local sidecar threat model (loopback bind, shared `default` scope) | Developer docs |
| **W3** | First [RESULTS.md](../../benchmarks/eval-suite/RESULTS.md) publish (Go + mock upstream) | Public proof |
| **W3** | API compatibility matrix (OpenAI + Anthropic streaming surface) | Semver policy draft |
| **W4** | Rust eval parity run (`benchmarks/run_rust_audit.sh`) | Dual-track confidence |
| **W4** | Gateway threat appendix: CIDR trust, header policy, socket validation | Platform reviewers |

**Exit criteria:**

- One reproducible eval command produces a fill-in RESULTS report
- Security reviewer can approve local sidecar without a call
- Documented semver stance for intercepted vs passthrough routes

---

## Weeks 5–8 — Observability & operator dashboards

**Primary (70%):** Developer-facing savings visibility during coding sessions  
**Secondary (30%):** Prometheus export ready for cluster aggregation

| Week | Deliverable | Owner focus |
|------|-------------|-------------|
| **W5** | Implement `/metrics` per [OBSERVABILITY-SPEC.md](../operations/OBSERVABILITY-SPEC.md) (Phase 1 counters) | Core engineering |
| **W5** | `korto_cache_hits_total`, `korto_requests_total`, `korto_redactions_total` | Minimum viable metrics |
| **W6** | Compressor + scope cardinality metrics; LRU eviction counters | Memory safety visibility |
| **W6** | Local dashboard: Grafana JSON or minimal bundled UI at `/dashboard` | Developer wedge |
| **W7** | VS Code extension: surface cache hit + estimated token savings in status bar | IDE integration |
| **W7** | Structured slog → optional JSON for local log aggregation | Power users |
| **W8** | SLO doc: p50/p99 from k6 + cancel-storm audit (`make cancel-audit`) | Production readiness |
| **W8** | OTel trace spans (optional flag) for request → upstream → cache path | Enterprise path |

**Exit criteria:**

- Developer answers "is Kotro saving me money?" from dashboard in < 60s
- Platform engineer can scrape `/metrics` into existing Prometheus
- Documented SLO baselines for cache hit and cancel-storm goroutine stability

---

## Weeks 9–13 — Design partners, GTM & enterprise blueprint

**Primary (70%):** 3 developer-heavy design partners  
**Secondary (30%):** Helm chart + sidecar → ingress migration guide

| Week | Deliverable | Owner focus |
|------|-------------|-------------|
| **W9** | Design partner outreach template + onboarding runbook (local sidecar) | GTM |
| **W9** | Partner #1 onboarded: Cursor / Claude Code / custom MCP agent team | Real workload |
| **W10** | Partner #2 onboarded: small consultancy with high agent call volume | ROI case study |
| **W10** | Publish workload-specific RESULTS (e.g. "MCP tool-heavy session") | Differentiation |
| **W11** | Partner #3 onboarded: security-conscious eng org | Trust validation |
| **W11** | Show HN / launch post anchored on eval data, not feature list | Community |
| **W12** | Helm chart: Deployment + Service + ConfigMap + optional ServiceMonitor | K8s ingress |
| **W12** | Architecture doc: sidecar → shared gateway promotion path | Enterprise upgrade |
| **W13** | v0.2.0 release: metrics + eval suite + partner case study | Milestone tag |
| **W13** | Monetization sketch (support / managed gateway / control plane) — no SKU required | Future revenue |

**Exit criteria:**

- ≥ 1 external quote with measured token savings %
- ≥ 100 GitHub stars or equivalent inbound interest signal
- Helm chart deploys Kotro as sidecar with Prometheus scrape working

---

## Workstream split (70 / 30)

| Workstream | Allocation | Key artifacts |
|------------|------------|---------------|
| **A — Developer wedge** | 70% | VS Code savings UI, agent eval fixtures, brew/npm polish, Show HN |
| **B — Enterprise upgrade** | 30% | Prometheus, Helm, gateway docs, partner security reviews |
| **C — Core quality** | Cross-cutting | Tests, Rust parity, cancel-storm gate on every release |

---

## Eval workload priorities (developer wedge)

Build fixtures that mirror real IDE/agent pain:

1. **Context reload storm** — same file tree / MCP schema re-injected every turn
2. **Tool output dumps** — large `grep`, `read_file`, terminal capture blocks across turns
3. **Deterministic replay** — identical prompts → cache HIT + byte-identical SSE
4. **Cross-credential isolation** — verify no HIT across different API keys
5. **Cancel storm** — 500 VUs disconnect mid-stream; goroutine leak gate

Scripts: `benchmarks/run_audit.sh`, `scripts/bench/k6-*.js`, `make bench`.

---

## Risk register

| Risk | Mitigation |
|------|------------|
| Solo bandwidth | Ship Phase 1 metrics only; defer OTel if needed |
| Partners won't share numbers | Offer anonymized aggregate publish |
| "Will it break my outputs?" | Parity hashes in eval suite; publish diffs |
| Security team blocks gateway mode | Lead with local sidecar + THREAT-MODEL; gateway as Phase 2 |
| Rust/Go drift | Weekly parity test in CI |

---

## Version milestones

| Target | Scope |
|--------|-------|
| **v0.1.3** | Eval harness + RESULTS template populated |
| **v0.2.0** | `/metrics`, dashboard, design partner case study |
| **v0.2.x** | Helm chart, gateway migration guide |

---

## Related documents

- [THREAT-MODEL.md](../security/THREAT-MODEL.md) — trust boundaries and isolation
- [OBSERVABILITY-SPEC.md](../operations/OBSERVABILITY-SPEC.md) — metrics framework
- [RESULTS.md](../../benchmarks/eval-suite/RESULTS.md) — benchmark dashboard template
- [RUST-ARCHITECTURE.md](../RUST-ARCHITECTURE.md) — dual-track implementation map
