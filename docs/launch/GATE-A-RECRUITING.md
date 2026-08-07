# Gate A — recruiting the remaining wedge (Sol 2026-08-07)

**Gate:** ≥3 teams **commit to trying** because they still need Kotro’s authority layer — not because containment is novel.  
**Status:** **Most urgent** next item after R4 claims. Containment is table stakes; do not lead with the R0.1a clip.

---

## Positioning (say this first)

> **Nono, sandbox-runtime (srt), or Docker enforce the boundary. Kotro makes the job authority portable, delegable, and provable.**

Cooperative, not competitive. Sit on top of ecosystem sandbox momentum.

**Remaining wedge (narrower than “signed permits”):**

| Wedge piece | Why it still matters after free vendor sandboxes |
|-------------|---------------------------------------------------|
| **Implemented delegation / attenuation** | Sub-agent / child jobs get narrowed authority, not a full copy of the parent badge |
| **Enforcement outside an instrumented hook** | OS/container boundary + mediator path — not only “framework called our hook” |
| **Source-pin / base-SHA binding** | Grants bound to a concrete repo identity + base revision |
| **Receipt spanning the job** | Offline-verifiable evidence across sandbox + model/tool mediation + workspace + landing |

**Not the differentiator alone:** “we have signed portable authority / receipts.”  
[Open Agent Passport (OAP)](https://github.com/aporthq/aport-spec) (and peers) already ship signed passports, Ed25519 checks on tool calls, signed decision records, fail-closed modes, and tamper-evident chains. Treat that as **architectural validation**, not proof the coding-agent case is solved. See [`KOTRO-PERMIT-POSITIONING.md`](../roadmap/KOTRO-PERMIT-POSITIONING.md) for OAP gap honesty.

---

## What to ask (the signal)

Ask teams that **already run a sandbox** (Claude Code sandbox, Docker, Nono, Pipelock, etc.):

1. Do you already run agents inside a sandbox?
2. If yes — do you still need **CI-issued task authority** (a signed job badge, not just “the process is jailed”)?
3. Do you need **delegation to sub-agents** with attenuated scopes?
4. Do you need **source-bound grants** (repo identity + `base_sha` / pin)?
5. Do you need **offline-verifiable evidence** spanning sandbox + mediation + land (not only a local log)?

| Outcome | Meaning |
|---------|---------|
| **≥3 teams say yes** to (2)–(5) in substance | Wedge is real — recruit into dogfood / Gate B |
| **Shrug / “sandbox is enough”** | That is the signal — stop further Permit engineering until the wedge is clearer; do not invent demand |

Interest without installation was the old bar. **Wedge-affirming teams willing to run against a real repo** is the new bar.

---

## What to show (secondary, not the lead)

Containment evidence remains useful as **proof of honesty** (ENOENT / mount-namespace absence), not as the product pitch.

| Asset | Use |
|-------|-----|
| R0.1a PASS | “Our Docker baseline prefers absence over path denial” — after wedge questions |
| [`PERMIT-GOLDEN-PATH.md`](./PERMIT-GOLDEN-PATH.md) | After they care about authority / receipts |
| [`PERMIT-ALPHA-CLAIMS.md`](./PERMIT-ALPHA-CLAIMS.md) | Before any public claim |

**Do not lead** with: poisoned README → `~/.ssh` → denied. Anthropic (and others) shipping free sandboxes makes that clip **table stakes**, not a Gate A closer.

**Do not claim from the clip alone:** unique signed authority industry-wide, “Kotro-only network,” hypervisor isolation, live draft-PR without host GitHub creds.

---

## Async ask copy (wedge-first)

```text
Quick question if you already sandbox Claude Code / Cursor agents:

Containment is getting commodity (vendor sandboxes, Docker, Nono, …).
We're testing whether teams still need a portable job badge on top:

  • CI-issued signed task authority
  • attenuated delegation to sub-agents
  • grants bound to repo + base SHA
  • offline-verifiable receipts across sandbox → mediation → land

Do any of those still hurt for you? If three teams say yes, we'll
dogfood Kotro Permit on a real repo. If not, that's useful too.

Docs (honest claims): https://github.com/kotro-labs/kotro-proxy-engine/blob/main/docs/launch/PERMIT-ALPHA-CLAIMS.md
```

---

## Runtime prerequisite (still call out)

Docker Desktop **≥ 4.x, native arch** for the current alpha backend.  
Other backends (Nono, Anthropic `sandbox-runtime`) are **adapters under contract** — see [`KOTRO-PERMIT-BACKEND-CONTRACT.md`](../roadmap/KOTRO-PERMIT-BACKEND-CONTRACT.md). Docker remains the **reference** until a spike passes the same acceptance rows.

---

## Parallelism

Gate A (wedge validation) is **ahead of** more Permit engineering.  
Do not start a Nono adapter beyond a **time-boxed spike** until Gate A signal is in.
