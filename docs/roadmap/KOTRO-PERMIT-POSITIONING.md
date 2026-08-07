# Kotro Permit — positioning (Sol 2026-08-07)

**Status:** Adopted. Supersedes any “nobody else has signed portable authority” framing.  
**Related:** [`../launch/GATE-A-RECRUITING.md`](../launch/GATE-A-RECRUITING.md) · [`../launch/PERMIT-ALPHA-CLAIMS.md`](../launch/PERMIT-ALPHA-CLAIMS.md)

---

## One-liner (cooperative)

> **Nono, sandbox-runtime (srt), or Docker enforce the boundary. Kotro makes the job authority portable, delegable, and provable.**

Sit **on top of** ecosystem sandbox momentum. Do not position Kotro as competing with free vendor sandboxes on containment alone.

---

## What is not our unique moat

**Do not claim:** Kotro is the only project with signed portable authority or signed receipts.

**Open Agent Passport (OAP)** already provides substantial overlap:

- Signed passport / credential
- Ed25519 verification on tool-call decisions
- Signed decision records
- Fail-closed authorization modes
- Tamper-evident hash chains

That is **strong architectural validation** of the authority+receipt idea — **not** proof that the coding-agent case is solved, and **not** a claim Kotro invented the category.

### OAP gaps (honest — use these, not “they don’t do signing”)

Per Sol’s read of OAP v1.0 / reference impl (keep updated; do not invent extras):

| Gap | Implication for Kotro |
|-----|------------------------|
| No formal **delegation chain** in v1.0 | Our non-expansion / attenuate path is a real wedge if we keep shipping it |
| Enforcement limited to **instrumented tool-call** boundaries | Shell/Python/child FS still need a sandbox backend outside the hook |
| Trusts the **framework to invoke the hook** | If the agent never hits the adapter, OAP doesn’t see the action |
| **ESCALATE** specified but unimplemented | Don’t claim escalate parity either side |
| **Single banking-domain** focus in published packs | Coding-agent source-pin / land / workspace is a different job |
| Spec author is founder of the company behind the reference impl | Treat as normal OSS conflict-of-interest disclosure, not an attack |

---

## Kotro’s remaining wedge (say only this)

Narrower and more specific than “signed permits”:

1. **Implemented delegation / attenuation** — child jobs get narrowed authority.
2. **Enforcement outside an instrumented hook** — Docker/Nono/srt boundary + Kotro mediator; not only “framework called us.”
3. **Source-pin / base-SHA binding** — grants tied to concrete repository identity + base revision.
4. **Receipt spanning the whole job** — sandbox + model/tool mediation + workspace + landing, offline-verifiable with trust store.

Containment alone is **table stakes** once Anthropic (and peers) ship sandboxes free. Gate A must test the wedge above — see Gate A doc.

---

## Claims hygiene

| Forbidden in public docs | Allowed |
|--------------------------|---------|
| “Nobody else has signed authority / receipts” | “OAP validates the category; our wedge is …” |
| “Codex ships Landlock + seccomp by default” | **Unverified** from secondary sources — **do not repeat** until primary docs cite it |
| “We replace Nono / Docker / Claude sandbox” | “They enforce the boundary; we issue the job badge” |
| Containment clip as the Gate A lead | Containment as secondary honesty evidence |

---

## Document history

| Date | Change |
|------|--------|
| 2026-08-07 | Sol correction adopted: OAP honesty, cooperative one-liner, narrow wedge |
