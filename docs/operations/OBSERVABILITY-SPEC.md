# Kotro Observability Specification

**Version:** 0.1.2 (spec) · Metrics implementation targeted v0.2.0  
**Strategy weighting:** 70% local developer visibility · 30% cluster Prometheus export

---

## 1. Goals

| Audience | Question | Interface |
|----------|----------|-----------|
| **Developer (sidecar)** | "Is Kotro saving tokens right now?" | Local dashboard / VS Code status |
| **Platform engineer** | "Is the proxy healthy in prod?" | `/metrics` + `/healthz` |
| **SRE** | "Are we leaking goroutines under cancel storms?" | pprof + audit gate |
| **Security** | "How much redaction is happening?" | Redaction counters |

---

## 2. Current state (shipped in v0.1.2)

| Capability | Endpoint / mechanism | Status |
|------------|---------------------|--------|
| Liveness | `GET /healthz` → `{"status":"ok"}` | **Shipped** |
| Structured logs | `slog` request logging (method, path, bearer presence) | **Shipped** |
| Cache hit signal | Response header `X-KortoLabs-Cache: HIT` | **Shipped** |
| Profiling | `/debug/pprof/*` when `KORTO_ENABLE_PPROF=true` | **Shipped** (dev/audit) |
| Prometheus metrics | `GET /metrics` when `KORTO_ENABLE_METRICS=true` | **Shipped** (P0) |
| OpenTelemetry traces | — | **Planned** (optional flag) |
| Bundled dashboard | `GET /dashboard` when `KORTO_ENABLE_METRICS=true` | **Shipped** (P1) |

---

## 3. Metrics endpoint

### 3.1 Exposure

```
GET /metrics
Content-Type: text/plain; version=0.0.4
```

- Enabled by default in v0.2.0 (no sensitive label values)
- Disable via `KORTO_ENABLE_METRICS=false` if needed
- Bind considerations: same as proxy listener (`KORTO_LISTEN_ADDR`)

### 3.2 Naming convention

Prefix: `korto_`  
Labels: low cardinality only — never include API keys, prompts, or tenant IDs.

---

## 4. Metric catalog

### 4.1 Request plane

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `korto_requests_total` | Counter | `provider`, `route`, `stream` | Total intercepted requests |
| `korto_request_duration_seconds` | Histogram | `provider`, `cache_status` | End-to-end handler time |
| `korto_upstream_duration_seconds` | Histogram | `provider`, `status_class` | Upstream round-trip (miss path) |
| `korto_request_body_bytes` | Histogram | `provider` | Incoming JSON body size |
| `korto_errors_total` | Counter | `provider`, `error_class` | `error_class`: `body_limit`, `upstream`, `parse`, `timeout`, `internal` |

`provider`: `openai` | `anthropic` | `passthrough`  
`cache_status`: `hit` | `miss` | `bypass` (cache disabled or non-streaming)  
`status_class`: `2xx` | `4xx` | `5xx` | `error`

### 4.2 Cache plane

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `korto_cache_hits_total` | Counter | `provider` | Semantic cache hits |
| `korto_cache_misses_total` | Counter | `provider` | Cache misses (upstream fetched) |
| `korto_cache_stores_total` | Counter | `provider` | New entries written |
| `korto_cache_replay_bytes_total` | Counter | `provider` | Bytes served from cache on hit |
| `korto_cache_entries` | Gauge | — | Approximate live entries (post-eviction sweep) |
| `korto_cache_evictions_total` | Counter | `reason` | `reason`: `ttl` | `manual` |

**Developer wedge UI mapping:**

- Hit rate = `hits / (hits + misses)`
- Est. latency saved ≈ `hits * (upstream_p50 - replay_p50)` (from histograms)

### 4.3 Compressor plane

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `korto_compressor_blocks_stripped_total` | Counter | — | Context blocks removed as duplicates |
| `korto_compressor_bytes_saved_total` | Counter | — | Estimated bytes not sent upstream |
| `korto_compressor_scopes_active` | Gauge | — | Current LRU entries |
| `korto_compressor_scope_evictions_total` | Counter | `reason` | `reason`: `lru` | `ttl` |

Backed by: `internal/compressor/context.go` (`hashicorp/golang-lru/v2/expirable`) and Rust `moka` equivalent.

### 4.4 Guardrail plane

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `korto_redactions_total` | Counter | `pattern` | Secrets replaced before upstream |
| `korto_redaction_restores_total` | Counter | — | Placeholders restored in stream |

`pattern`: coarse bucket — `aws_key`, `api_key`, `email`, `connection_string`, `sk_token`, `other`  
Never emit the matched secret or placeholder value.

### 4.5 Scope & isolation plane

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `korto_scope_mode_total` | Counter | `mode` | `mode`: `credential` | `gateway_header` | `default` |
| `korto_trusted_peer_rejections_total` | Counter | — | Gateway headers ignored (untrusted peer) |

**Cardinality guard:** No per-tenant labels. Use aggregate counters only.

### 4.6 Runtime plane

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `korto_goroutines` | Gauge | — | `runtime.NumGoroutine()` sampled periodically |
| `korto_process_resident_memory_bytes` | Gauge | — | From `runtime.MemStats` |

For deep leak analysis, continue using `KORTO_ENABLE_PPROF=true` + `make cancel-audit`.

---

## 5. Histogram buckets (recommended)

```text
request_duration_seconds:  0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10
upstream_duration_seconds: same
request_body_bytes:        1024, 4096, 16384, 65536, 262144, 1048576, 5242880, 10485760
```

---

## 6. Local developer dashboard (70% priority)

### 6.1 Minimal bundled UI (`/dashboard`)

Single-page view refreshed every 5s:

| Panel | Source |
|-------|--------|
| Cache hit rate (last 5m) | `korto_cache_hits_total` / misses |
| Tokens / bytes saved (compressor) | `korto_compressor_bytes_saved_total` delta |
| Redactions this session | `korto_redactions_total` delta |
| Active compressor scopes | `korto_compressor_scopes_active` |
| Last 10 requests | ring buffer in memory (path + cache status only) |

### 6.2 VS Code extension integration

**Shipped in extension v0.2.0:**

- Status bar: last cache result + session compressor bytes saved
- Click status bar → opens `http://localhost:8080/dashboard`
- Polls `/api/dashboard` every 5s (falls back to `/healthz` when metrics API unavailable)

---

## 7. Cluster / Prometheus (30% priority)

### 7.1 ServiceMonitor (Helm, week 12)

```yaml
spec:
  endpoints:
    - port: http
      path: /metrics
      interval: 15s
```

### 7.2 Recommended alerts

| Alert | Expression (sketch) | Severity |
|-------|---------------------|----------|
| High error rate | `rate(korto_errors_total[5m]) / rate(korto_requests_total[5m]) > 0.05` | warning |
| Cache hit rate drop | `hit_rate < 0.1` for 30m (baseline-dependent) | info |
| Scope cardinality spike | `korto_compressor_scopes_active > 8000` | warning |
| Goroutine growth | `deriv(korto_goroutines[10m]) > 10` | critical |

---

## 8. OpenTelemetry (optional, v0.2.x)

Env: `KORTO_ENABLE_OTEL=true`, `OTEL_EXPORTER_OTLP_ENDPOINT`

| Span | Attributes |
|------|------------|
| `korto.request` | `provider`, `stream`, `cache_status` |
| `korto.upstream` | `http.status_code`, `duration_ms` |
| `korto.compress` | `blocks_stripped`, `bytes_saved` |
| `korto.redact` | `redaction_count` |

No prompt content, credentials, or tenant IDs on spans.

---

## 9. SLO baselines (document, not enforce)

Populate from `make cancel-audit` and `make load-test`:

| SLO | Target (initial) | Measurement |
|-----|------------------|-------------|
| Availability | 99.9% `/healthz` | Synthetic probe |
| Cache hit latency p99 | < 50ms | `korto_request_duration_seconds{cache_status="hit"}` |
| Upstream miss p99 | Provider-dependent | `korto_upstream_duration_seconds` |
| Cancel-storm stability | Goroutine delta ≤ 5 | `benchmarks/run_audit.sh` |
| Error rate | < 1% under load | k6 mixed scenario |

Publish results in [benchmarks/eval-suite/RESULTS.md](../../benchmarks/eval-suite/RESULTS.md).

---

## 10. Implementation phases

| Phase | Version | Scope |
|-------|---------|-------|
| **P0** | v0.2.0 | `/metrics` counters + histograms for request/cache/compressor/redaction |
| **P1** | v0.2.0 | Bundled `/dashboard` + VS Code status bar |
| **P2** | v0.2.x | OTel opt-in, ServiceMonitor, Grafana JSON |
| **P3** | v0.3.0 | Token estimator (model-aware), per-session savings export |

---

## 11. Privacy rules for observability

1. **Never** label metrics with tenant ID, session ID, API key hash, or prompt text
2. **Never** log request/response bodies at INFO level
3. Redaction pattern labels are coarse buckets only
4. pprof enabled only on trusted interfaces
5. Dashboard request ring buffer stores method + path + cache status only

---

## 12. Related documents

- [THREAT-MODEL.md](../security/THREAT-MODEL.md) — pprof and data sensitivity
- [90-DAY-ROADMAP.md](../roadmap/90-DAY-ROADMAP.md) — weeks 5–8 delivery plan
- [RESULTS.md](../../benchmarks/eval-suite/RESULTS.md) — SLO evidence template
