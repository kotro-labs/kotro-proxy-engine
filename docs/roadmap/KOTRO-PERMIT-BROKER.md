# Kotro Permit — GitHub broker & agent↔Kotro PR protocol

**Status:** Design for implementation (alpha thin broker + R3 hardening)  
**Audience:** Implementers, CLI/docs authors  
**Related:**  
- [`KOTRO-PERMIT-README.md`](./KOTRO-PERMIT-README.md) — index / review pack  
- [`KOTRO-PERMIT-TASKS.md`](./KOTRO-PERMIT-TASKS.md) — execution order  
- [`KOTRO-PERMIT-SANDBOX.md`](./KOTRO-PERMIT-SANDBOX.md) — sandbox, resources, landing UX  
- [`KOTRO-PERMIT-AUTHORITY.md`](./KOTRO-PERMIT-AUTHORITY.md) — signed permit lifecycle  

**Last updated:** 2026-08-06

---

## 1. Product decision (locked for planning)

| Decision | Choice |
|----------|--------|
| Primary land UX | **Kotro opens a draft PR** (modern AI expectation) — not “user must manually open PR” |
| Where GitHub token lives | **Kotro host broker only** — never in agent env/mounts |
| Agent role | Sends **intent** (“open draft PR”) + artifact identity — never talks to GitHub |
| Default permit max | **`draft_pr` only** — never `merge` |
| Human gate (alpha) | **Allow-once** confirm before push/PR |
| Heavy crypto handshake | **Not required** — use **run-scoped token** minted at container start |
| Sequencing | **After R0.1a passes** → thin broker in **alpha (R2.x)**; **R3** = attenuation, receipts, polish |
| Reject | `GITHUB_TOKEN` inside Docker “like Claude/Codex” |

**Positioning sentence:**  
*Like other coding agents we open a draft PR for you — unlike them, the agent never holds your GitHub token; Kotro does, after you confirm.*

---

## 2. Why (market vs Permit)

| Pattern | Typical AI tools | Kotro Permit |
|---------|------------------|--------------|
| Speed to PR | Agent pushes / `gh pr create` | Broker opens **draft** PR after allow-once |
| Credentials | Often in agent env | **Host broker only** |
| Merge | Sometimes autonomous | **Always human** in default permit |
| Untrusted repo | Weak FS/net story | Sealed copy + deny-all + permit |

Manual “you open the PR” is too slow for 2026. Matching their **speed** without matching their **token-in-agent** threat model is the wedge.

---

## 3. High-level flow

```text
Agent (Docker)              Kotro broker (host)              You           GitHub
     │                              │                         │              │
     │  has KOTRO_RUN_TOKEN only    │                         │              │
     │  (NOT GITHUB_TOKEN)          │  holds GITHUB_TOKEN     │              │
     │                              │                         │              │
     │  POST open draft PR          │                         │              │
     │  + artifact hash             │                         │              │
     │─────────────────────────────>│                         │              │
     │                              │ validate run token      │              │
     │                              │ permit allows draft_pr  │              │
     │                              │ allow-once ────────────>│              │
     │                              │<──────── yes ───────────│              │
     │                              │ push + create draft ──────────────────>│
     │                              │<─────────────── pr_url ────────────────│
     │  { pr_url }                  │                         │              │
     │<─────────────────────────────│                         │              │
     │                              │                         │  you merge   │
```

User-visible steps stay ≤ 3: **Run → Review (diff / allow-once) → Merge on GitHub.**

---

## 4. Communication channel

### Wire

- Agent reaches **Kotro data-plane only** (dual-homed / `internal: true` agent net).
- Same plane as LLM proxy; add broker routes (or MCP tool that calls them).
- Agent **must not** reach GitHub, Kotro control-plane, or `docker.sock`.

```text
Agent container ──HTTP──► Kotro data-plane
                    ├─ /v1/...              LLM (existing)
                    └─ /v1/broker/...       draft PR (new)
                       or MCP: open_draft_pr → same handler
```

### API shape (either is fine)

| Shape | Notes |
|-------|--------|
| `POST /v1/broker/draft-pr` | Simple to log, test, auth |
| MCP tool `open_draft_pr` | Better agent ergonomics; **same** auth underneath |

Do **not** invent a second bus. Do **not** use HTTP CONNECT to GitHub through the proxy as the PR mechanism — prefer an explicit broker that can **only** perform allowed land actions.

---

## 5. “Handshake” — what is required

### Not required (alpha)

- Custom multi-round crypto protocol beyond normal transport
- mTLS / workload certificates (optional later)
- Agent-side GitHub OAuth
- Human login *inside* the container

### Required (alpha) — run ticket + allow-once

This **is** the handshake in substance:

| Step | When | What |
|------|------|------|
| **Mint run token** | `kotro run --permit` start | Opaque `KOTRO_RUN_TOKEN` bound to `run_id` + `permit_digest` + expiry |
| **Inject** | Into container env | `KOTRO_BROKER_URL` + `KOTRO_RUN_TOKEN` only — **never** `GITHUB_TOKEN` |
| **Call** | Agent wants PR | `Authorization: Bearer <run_token>` + artifact identity |
| **Validate** | Broker | Token valid, permit has `draft_pr`, run not expired |
| **Allow-once** | Before push | Human confirms in CLI/UI |
| **Artifact bind** | Before push | PR contents match reviewed diff / staging commit hash |
| **Execute** | Host | Kotro uses host GitHub credentials → draft PR |
| **Invalidate** | On run end / TTL | Run token useless after |

```text
START (once)                         PR TIME
────────────                         ───────
Verify permit                        POST + run_token + artifact hash
Mint KOTRO_RUN_TOKEN ──────────────► Validate → allow-once → host GitHub
Inject into container                Return pr_url
```

### Binding layers

| Layer | Mechanism | Alpha | Purpose |
|-------|-----------|-------|---------|
| L0 | Network: agent → data-plane only | Required | Topology |
| L1 | Run token (`run_id` + `permit_digest` + TTL) | **Required** | Authn of caller |
| L2 | Permit capability `draft_pr` (not `merge`) | **Required** | Authz |
| L3 | Human allow-once | **Required** | Speed with brake |
| L4 | Artifact hash bind | **Required** | No bait-and-switch |
| L5 | mTLS / workload identity | Later | Harden |
| L6 | Signed receipts | R3 | Evidence |

### Allow-once anti-fatigue (Fable flag — required UX)

L3 fails if humans press `y` reflexively after several iterations. Test #23 only proves denial works — **not** that humans still read the prompt.

**Required allow-once prompt contents (alpha):**

| Show | Why |
|------|-----|
| File count + lines added/removed | Instant scale of change |
| Artifact / diff hash (short) | Ties to L4 |
| **Highlight if diff touches execution-bearing paths** | e.g. `.github/workflows/**`, `.git/hooks/**`, `**/package.json` lifecycle fields, `.vscode/tasks.json`, `Makefile`, `.envrc` — tired humans still notice “touches CI” |
| Title / head → base | Orientation |
| Explicit: draft only, no merge | Expectation |

**Rules:**

- New artifact hash ⇒ prior allow-once **invalid** (must re-prompt).  
- Same hash re-request within run ⇒ may show “already approved this artifact” without re-rubber-stamping a *different* diff.  
- Never batch-approve “all future PRs this run” in alpha default.  
- Optional later: single allow for “update existing draft PR with new artifact” still shows the highlight summary.

Implementers: treat prompt content as part of the security control, not cosmetic CLI chrome.

---

## 6. Alpha (thin broker) vs R3 (harden)

| Capability | Alpha (R2.x) thin broker | R3 harden |
|------------|--------------------------|-----------|
| Agent → Kotro intent | Yes | Yes |
| Host holds GitHub token | Yes | Yes |
| Allow-once draft PR | Yes | Yes |
| Run token + artifact hash | Yes | Yes |
| Draft only / no merge | Yes | Yes |
| Token attenuation / fine scopes | Basic or PAT-as-configured | Hardened |
| Signed land receipts | Optional / stub | Yes |
| Abuse / rate / multi-repo polish | Minimal | Yes |
| `draft_pr: auto` without confirm | **No** (default) | Explicit opt-in only |

**Gate:** Do not start thin broker implementation until **R0.1a containment spike passes** (with DNS-fixed #6).  
Containment is the kill-shot; PR UX must not distract from a failed thesis.  
**Sol:** Do not start R2-B until P0.3 clean-git + P1.1 permit-bound refs are specified and accepted ([SOL-REVIEW](./KOTRO-PERMIT-SOL-REVIEW.md)).

---

## 6b. Host git land contract (Sol P0.3 — before any push)

The agent-writable staging tree is **untrusted**, including `.git`.

| Rule | Requirement |
|------|-------------|
| `.git` metadata | **Never** consume agent-controlled `.git` (config, hooks, helpers, refs) |
| Materialize | Copy approved **tree** into a **clean, host-owned** repository |
| Hooks / config | Disable hooks; no inherited user gitconfig influencing the land |
| Push object | Exact **immutable tree/commit** matching approved artifact hash |
| Branch names | Kotro **generates** head branch; ignore agent remotes/refspecs |
| Base | From **permit** (canonical repo + allowed base ref/revision) — not agent request |
| Revalidate | After allow-once, **immediately before** side effect: permit, run token, artifact hash, approval; fail if expired meanwhile |
| Disclosure | Draft PR / push may trigger **CI, bots, `pull_request` apps** — draft ≠ no execution |

Request sketch fields `head_branch` / `base_branch` are **hints at most** — not authoritative.

---

## 7. Request / response (sketch for implementers)

Illustrative only — finalize field names in R0.2 / R2.x.

**Request (agent → Kotro):**

```http
POST /v1/broker/draft-pr
Authorization: Bearer <KOTRO_RUN_TOKEN>
Content-Type: application/json

{
  "title": "…",
  "body": "…",
  "head_branch": "kotro/…",
  "base_branch": "main",
  "artifact": {
    "kind": "staging_commit",
    "hash": "<git sha or diff digest>"
  }
}
```

**Broker checks:**

1. Run token valid and not expired  
2. Permit allows `draft_pr`, denies `merge`  
3. Artifact hash matches Kotro materialization (**clean host-owned repo**, not agent `.git`)  
4. Allow-once granted for this artifact (anti-fatigue prompt shown)  
5. **Revalidate** permit/token/hash/approval immediately before push  
6. Repo/base from **permit**, not agent-supplied remotes/refs  
7. Host GitHub credentials present — else fail closed with clear error  

**Success:** `{ "pr_url": "https://github.com/…/pull/N", "draft": true }`  
**Failures:** `unauthorized` | `permit_denied` | `allow_once_required` | `artifact_mismatch` | `github_unconfigured` | `expired` — never fall back to injecting token into agent.

---

## 8. Credential rules (non-negotiable)

| Secret | Agent container | Kotro host |
|--------|-----------------|------------|
| `KOTRO_RUN_TOKEN` | Yes (short-lived) | Mints/validates |
| `GITHUB_TOKEN` / git creds | **Never** | Yes |
| Provider LLM API key | **Never** | Yes (existing proxy) |
| Permit private key | **Never** | Host only |

Acceptance: extend suite so broker path proves **#10-class** — GitHub token absent from agent env and mounts even when draft PR succeeds.

---

## 9. Workflow fit

| Workflow | Alpha path |
|----------|------------|
| Code / in-repo docs / design-as-files | Agent edits copy → allow-once → **draft PR** → you merge |
| Confluence / Notion / Figma | **Out of scope** — no SaaS brokers in alpha; do outside or later broker |

---

## 10. Anti-patterns (do not implement)

- [ ] `GITHUB_TOKEN` in container for `gh pr create`  
- [ ] Agent merge-to-main capability in default permit  
- [ ] Broker call without run token  
- [ ] Allow-once without artifact hash check  
- [ ] Silent push with no human confirm (alpha default)  
- [ ] Teaching users to `docker exec` + push  
- [ ] Claiming brokered PR before R0.1a / thin broker actually works  
- [ ] Blocking alpha forever on full R3 attenuation/receipts  

---

## 11. Implementer checklist

### Thin broker (alpha)

- [ ] Mint/inject `KOTRO_RUN_TOKEN` at `run --permit` start; TTL + bind to permit digest  
- [ ] Broker route or MCP tool on **data-plane only**  
- [ ] Validate token + `draft_pr` capability  
- [ ] Allow-once UX with **anti-fatigue summary** (file/line counts + execution-bearing path highlights)  
- [ ] Materialize approved tree into **clean host-owned repo**; never agent `.git`  
- [ ] Permit-bound repo/base; Kotro-generated head branch  
- [ ] Artifact hash bind + **revalidate immediately before push**  
- [ ] Disclose CI/bot execution on draft PR  
- [ ] Host-side git push + draft PR (`gh` or GitHub API)  
- [ ] Return `pr_url` to agent/user  
- [ ] Clear errors; fail closed; no host-unsandboxed fallback  
- [ ] Tests: token not in agent; forged run token; artifact mismatch; allow-once deny; post-expiry push deny  
- [ ] User docs: positioning sentence + allow-once + CI disclosure  
- [ ] Do not block R2-A thesis (diff→apply) on finishing R2-B broker  

### R3 harden (after Gate B)

- [x] Attenuated tokens / least privilege (`draft_pr` scope, TTL, one-shot land)
- [x] Signed land receipts + `permit_digest` + `receipt verify --trust` levels
- [x] Escape Lab rows for broker abuse cases (EL-31–34 planned; unit coverage)
- [ ] Optional explicit auto-draft (never default)

---

## 12. Document history

| Date | Change |
|------|--------|
| 2026-08-06 | Initial: thin broker in alpha, run-token handshake, allow-once, R3 harden split |
| 2026-08-06 | Fable: anti-fatigue allow-once prompt; staged R2-A/R2-B note |
| 2026-08-06 | Sol: §6b clean host git; permit-bound refs; CI disclosure; revalidate-before-push |
| 2026-08-06 | Sol: §6b clean host git; permit-bound refs; CI disclosure; revalidate-before-push |
