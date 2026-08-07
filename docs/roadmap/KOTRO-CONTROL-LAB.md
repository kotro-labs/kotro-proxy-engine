# Kotro Control Lab — vendor-neutral verification suite

**Status:** Adopted direction (2026-08-07). Replaces Permit expansion as the primary open-source bet.  
**Thesis:** The ecosystem has sandboxes, scanners, tracers, and protocol conformance. It lacks a dominant, maintained suite that answers:

> Using this real coding agent on this OS, what does this runtime actually **stop**, what does it only **detect**, what **benign** work does it break, and what **overhead** does it add?

**Not in scope:** another sandbox, generic MCP gateway/scanner, tracing/replay dashboard, agent behavior-diff framework, IFC/taint stack, fashionable A2A, more Permit/receipt/delegation work.

---

## Positioning

Kotro **invokes** Nono, Anthropic sandbox-runtime, Docker, unsandboxed baseline, and Kotro controls as **test backends**. It does not replace them.

```text
Control Lab scenario
        │
        ├── none (unsandboxed baseline)
        ├── docker
        ├── nono
        ├── anthropic-srt
        └── kotro
                │
                └── comparable evidence (JSON / JUnit / Markdown)
```

Nono will often win FS/process containment. Kotro may win MCP schema drift, LLM-path redaction, circuit/budget, cross-plane evidence. **Publish both** — trust > “universally safer.”

---

## CLI sketch (target)

```bash
kotro-proxy control-lab run --backend none
kotro-proxy control-lab run --backend docker
kotro-proxy control-lab run --backend nono
kotro-proxy control-lab run --backend anthropic-srt
kotro-proxy control-lab run --backend kotro
```

Same scenarios every time. No simplistic single “security score” at first — **capability matrix** only.

---

## Scenario classes (v1 suite ~12)

**Hostile**

1. Read outside authorized repository  
2. Write outside working directory  
3. Access `~/.ssh` / cloud creds / host canary  
4. Connect to unauthorized network destination  
5. DNS + encoded-secret exfiltration attempts  
6. Change MCP tool schema after approval  
7. Inject instructions via MCP tool result  
8. Repeat failing tool call until budget/loop limit  

**Benign (false-positive / breakage)**

9. Build  
10. Test  
11. Read dependencies  
12. Edit an allowed file  
13. Access an approved API  

(Exact IDs TBD; fold existing Escape Lab rows where they map.)

---

## Report columns (per backend × scenario)

| Field | Values |
|-------|--------|
| Outcome | `prevented` \| `detect_only` \| `allowed_by_design` \| `bypassed` |
| Benign | `completed` \| `broken` \| `n/a` |
| False positive | where measurable |
| Overhead | cold-start, p50, p95 |
| Context | OS, arch, backend version, policy id |
| Evidence | reproduction command + raw artifacts |

---

## Adapter contract (keep tiny)

| Step | Meaning |
|------|---------|
| `prepare` | Policy / image / pins |
| `launch` | Start runtime |
| `execute` | Run scenario payload |
| `observe` | Collect prevent/detect/allow + logs/latency |
| `cleanup` | Tear down |

Pin backend versions. Pre-1.0 APIs = scheduled recheck cost (same discipline as the frozen Permit backend note).

---

## Sequence

### Week 0 — Freeze Permit ✅ (this decision)

- Permit = experimental, code-frozen ([`KOTRO-PERMIT-README.md`](./KOTRO-PERMIT-README.md))  
- Security/correctness fixes only  
- No R2+/broker/delegation/receipt/adapter product work  

### Week 1 — Make present evidence credible

- Implement Escape Lab **v2 scoreboard** renderer ([`../security/ESCAPE-LAB-SCOREBOARD.md`](../security/ESCAPE-LAB-SCOREBOARD.md))  
- Add benign controls, FP, latency rollups  
- Official **MCP conformance** runner as CI table-stakes  
- Reassess regex injection on benign security-writing samples; label heuristic if precision is weak  

### Weeks 2–3 — Extract neutral harness

Backends first: `none`, `kotro`, `nono`, `anthropic-srt` (+ Docker when cheap).  
Outputs: JSON, JUnit, Markdown — **no web dashboard**.  

### Week 4 — Validate publicly

Publish one honest report:

> We ran the same ~12 agent-control tests against Nono, Anthropic sandbox-runtime, Docker, and Kotro. Here is what each stopped, allowed, and broke.

Ask maintainers to correct policies/assumptions before calling it definitive.

**Continue only if within ~4 weeks:**

- ≥3 external users reproduce the suite  
- ≥1 external maintainer engages  
- Someone contributes/requests a backend or scenario  
- Recurring ask for comparison or CI regression  

Otherwise **stop expanding** the harness. Do not invent a six-month roadmap from weak interest.

---

## Explicit non-goals

| Direction | Recommendation |
|-----------|----------------|
| Another native sandbox | No |
| Generic MCP gateway/scanner | No |
| Generic tracing/replay dashboard | No |
| Agent behavior-diff framework | No |
| Information-flow / taint framework | No (FIDES et al.) |
| A2A because fashionable | Not until a real user workflow |
| More Permit / receipts / delegation | **Frozen** |
| Vendor-neutral Control Lab | **Yes** |
| MCP conformance + Escape Lab accuracy | **Yes** (maintenance) |

---

## Success metric

External **reproduction and contribution**, not another large subsystem.

---

## Ratings (adopted)

| Lens | Score |
|------|-------|
| Engineering quality | 8/10 |
| Current differentiation | 6/10 |
| Open-source usefulness | 7/10 |
| Adoption evidence | 3/10 |
| Potential as neutral verification project | 8/10 |

---

## Related

- Escape Lab matrix (live): [`../security/ESCAPE-LAB-MATRIX.md`](../security/ESCAPE-LAB-MATRIX.md)  
- Scoreboard design: [`../security/ESCAPE-LAB-SCOREBOARD.md`](../security/ESCAPE-LAB-SCOREBOARD.md)  
- Permit freeze: [`KOTRO-PERMIT-README.md`](./KOTRO-PERMIT-README.md)  

## Document history

| Date | Change |
|------|--------|
| 2026-08-07 | Adopted: freeze Permit; Control Lab as primary OSS direction |
