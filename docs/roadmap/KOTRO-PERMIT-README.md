# Kotro Permit — **FROZEN** (experimental)

**Status (2026-08-07):** **Code-frozen / experimental.**  
**Decision:** Do **not** continue Permit product work (R2-A expansion, broker expansion, delegation, receipts polish, Nono/srt Permit adapters) until users **explicitly ask**.

Security and correctness fixes only. Do not delete tests or docs.

---

## Why freeze

The ecosystem already has sandboxes (Nono, Anthropic sandbox-runtime, Docker), signed authority peers (OAP), scanners, tracers, and MCP conformance. Kotro’s next open-source bet is **vendor-neutral measurement** — see [`KOTRO-CONTROL-LAB.md`](./KOTRO-CONTROL-LAB.md) — not another Permit subsystem.

Sunk cost is not a reason to continue. R0–R4 alpha code remains as a reference; it is **not** the star/fork roadmap.

---

## What stays

| Keep | Notes |
|------|--------|
| `run --permit` fail-closed gates | Tests remain green |
| Option A / stage / apply / thin broker / receipts | Experimental; no feature expansion |
| Dogfood spikes | Evidence artifacts only |
| Design pack below | Historical + freeze banner |

## What stops

| Stop | Until |
|------|--------|
| Broker expansion, live-PR productization | Explicit user demand |
| Delegation / attenuate productization beyond what exists | Explicit user demand |
| Nono / Anthropic **Permit** adapters | Explicit user demand (Control Lab may still test those runtimes as **backends**) |
| Gate A Permit recruiting as primary engineering driver | Replaced by Control Lab validation gates |

---

## Honest product claim while frozen

> Permit is **experimental**. Prefer Escape Lab / Control Lab for comparing what runtimes actually stop. Do not market Permit as production agent governance.

Claims matrix (still valid for honesty if someone finds the code): [`../launch/PERMIT-ALPHA-CLAIMS.md`](../launch/PERMIT-ALPHA-CLAIMS.md) — lead with **experimental / frozen**.

---

## Design pack (archived index)

1. [`KOTRO-PERMIT-POSITIONING.md`](./KOTRO-PERMIT-POSITIONING.md)  
2. [`KOTRO-PERMIT-BACKEND-CONTRACT.md`](./KOTRO-PERMIT-BACKEND-CONTRACT.md) — Control Lab may reuse adapter ideas; **Permit** adapters frozen  
3. [`KOTRO-PERMIT-TASKS.md`](./KOTRO-PERMIT-TASKS.md)  
4. Rest of pack unchanged historically  

**Next repo direction:** [`KOTRO-CONTROL-LAB.md`](./KOTRO-CONTROL-LAB.md).
