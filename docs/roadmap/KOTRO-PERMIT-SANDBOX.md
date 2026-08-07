# Kotro Permit — Sandbox, isolation & resource model

**Status:** Design constraints for implementation + required user disclosures  
**Audience:** Implementers (R0–R2), docs/CLI copy authors, reviewers  
**Related:**  
- [`KOTRO-PERMIT-README.md`](./KOTRO-PERMIT-README.md) — index / review pack  
- [`KOTRO-PERMIT-TASKS.md`](./KOTRO-PERMIT-TASKS.md) (v7 task list)  
- [`KOTRO-PERMIT-AUTHORITY.md`](./KOTRO-PERMIT-AUTHORITY.md) — signed short-lived permit  
- [`KOTRO-PERMIT-BROKER.md`](./KOTRO-PERMIT-BROKER.md) (draft-PR protocol / run token)  
**Last updated:** 2026-08-06

This document captures decisions and open constraints discussed before implementation.  
**Do not overclaim.** Prefer narrow, true sentences in README/CLI/docs.

---

## 1. What we are building (product vs mechanism)

| Concept | Meaning |
|---------|---------|
| **Sandbox** | *Where* the agent runs — OS/container confinement (locked room) |
| **Permit** | *What* the agent is allowed to be for this job — signed, short-lived task-scoped authority (badge) |
| **Kotro (mediator)** | Controlled window out of the room — LLM proxy + **GitHub draft-PR broker** (alpha thin; R3 harden) |

**One-line product sentence (alpha):**  
Nono, srt, or Docker enforce the boundary; Kotro makes the job authority portable, delegable, and provable — land via apply or a Kotro-brokered draft PR you confirm (agent never holds the GitHub token).

**Not the same as “agent in a sandbox” alone.** Vendor sandboxes are table stakes. Permit’s differentiation is **authority + delegation + source-bound grants + spanning receipts** — enforced *with* a real sandbox backend (see positioning doc).

### Current Kotro vs Permit

| | Current Kotro | Kotro Permit |
|--|---------------|--------------|
| Metaphor | Watches traffic / tool calls Kotro sees | Sealed room + badge + controlled window |
| Agent process | Often on host | Inside sandbox for `run --permit` |
| Secret via shell/Python | Often still possible | Must fail (acceptance #4/#5) |
| Fail mode | Can be audit/soft | Fail-closed: no backend → refuse to start |

Permit **reuses** Kotro as mediator; it does not replace the need for confinement.

---

## 2. Isolation claims (honest wording)

### What we can claim (when wired correctly)

- Host paths **not mounted** into the agent are unreachable (prefer denial via **ENOENT** / `FileNotFoundError`, not “mounted but Permission denied”).
- Agent **public** egress denied under Docker `internal: true` (baseline). **Not** sufficient alone for “Kotro data-plane only” — R0.1b requires host canary + gateway scan; firewall/bind rules in scope for that claim (SOL-REVIEW P0.2).
- Alpha host write path: **ephemeral repo copy** → reviewed artifact → **Kotro-brokered draft PR** (or apply); you merge.
- Provider/Git tokens **not** in agent env/mounts (Kotro/broker holds them).
- No `docker.sock` / host-control socket in the agent sandbox.
- Runtime image contents (`/usr`, certs, etc.) are **trusted execution material**, not “project data grants.”

### What we must not claim

- “Complete isolation” / “kernel-proof” / “unbreakable” while the alpha backend is Docker/OCI.
- “We mask all dangerous files” on a live workspace (Option B) — execution-bearing files are an **open-ended** set.
- That post-start symlinks to **any** path dangle — only **unmounted host** targets; image paths like `/etc/passwd` or `/proc/self/environ` may resolve.
- That a containment spike demo implies permits, signatures, or receipts already exist (label: **Containment feasibility spike**).
- Hardened R3 broker features (attenuation, signed land receipts) before they exist — thin draft-PR broker is an alpha claim **only when R2.5 is implemented**.
- That Confluence/Figma updates work under Permit before those brokers exist.

### Symlink claim (canonical)

> Host paths outside the mount set are unreachable. Paths inside the runtime image remain readable and are classified as trusted execution material, not user data.

---

## 3. Sandbox backend strategy

### Alpha default: Docker / OCI container

**Yes — a container is planned** for the agent under `kotro-proxy run --permit`.

| Decision | Choice |
|----------|--------|
| Backend (alpha) | Docker Engine / Docker Desktop (OCI) — **reference**; keep until adapters pass contract |
| Lifecycle | **Per permit run** (start with task, tear down on exit) — not an always-on agent VM |
| If Docker unavailable | **Refuse to start** (fail-closed); never fall back to unsandboxed host |
| Who runs where | Agent **in** container; Kotro mediator **outside** (host / dual-homed); keys outside agent |
| Alternates | Time-boxed **Nono** spike + **srt** only with `failIfUnavailable=true` hard-required — [`KOTRO-PERMIT-BACKEND-CONTRACT.md`](./KOTRO-PERMIT-BACKEND-CONTRACT.md) |
| Property | Prefer **mount-namespace absence** (ENOENT) over path denial alone |
| Network | Agent on **internal** network; Kotro dual-homed to upstream |
| Repo mount | **Option A:** ephemeral copy only |
| Later escape hatch | `--live-workspace`, explicitly labeled **lower security** |

#### Intended run lifecycle

```text
kotro-proxy run --permit <envelope> -- <agent…>

1. Verify permit (signature, expiry, trust)
2. Require sandbox backend available → else exit fail
3. Stage ephemeral copy of allowed project data
4. Mint KOTRO_RUN_TOKEN; create networks + start container
5. Inject broker URL + run token only (never GITHUB_TOKEN)
6. Run agent command inside container
7. Agent may request draft PR via Kotro broker (see broker doc)
8. Allow-once + artifact bind → host creates draft PR
9. Tear down container; cleanup/retain staging per policy
10. You merge on GitHub
```

#### Topology (target)

```text
HOST
├─ Real ~/.ssh, other secrets, live checkout     ← not mounted into agent
├─ Ephemeral staging copy                        ← prepared by Kotro
├─ kotro-proxy (data-plane; holds API keys)
└─ Docker engine

AGENT CONTAINER (this run only)
├─ Runtime image (trusted execution material)
├─ Mount: ephemeral project copy
├─ Network: deny-all to public internet
└─ May reach: Kotro data-plane only
     (not control-plane / dashboard; not docker.sock)
```

#### Relation to today’s `isolate docker`

Current code **emits** compose/profiles; it does not fully **execute** agent confinement.  
Permit requires: **start sandbox or refuse** — profile generation alone is not enough.

### Why not VM-first (and when VM is better)

Containers **share a guest kernel** (on Linux hosts, the host kernel). They do **not** provide complete hypervisor-level separation.

| | Docker/OCI (alpha) | VM / microVM (later tier) |
|--|--------------------|---------------------------|
| Unmounted host secrets | Enforceable | Enforceable |
| Deny-all egress + mediator | Enforceable | Enforceable |
| Kernel escape resistance | Weaker | Stronger |
| Laptop UX / iterate speed | Better for alpha | Heavier |
| Multi-tenant / compliance language | Weaker | Stronger |

**Mac note:** Docker Desktop already runs Linux VMs; agent containers sit inside that VM. Escape-to-macOS ≠ escape-to-Linux-guest-kernel. Still do not market Docker tier as “complete kernel isolation.”

**Recommendation (locked for planning):**

1. Alpha backend = Docker.  
2. Architecture = **pluggable** `SandboxBackend` (`DockerBackend`, later `MicroVmBackend`, optional vendor sandbox).  
3. Product tiers with honest labels:

| Tier | Backend | User-facing claim |
|------|---------|-------------------|
| Default / alpha | Docker | Workspace + network confinement under a permit |
| Strong (future) | MicroVM / VM | Same + hypervisor isolation |
| `--live-workspace` | Either | Explicitly **lower security** |

**Implementers:** do not hard-wire assumptions that prevent a second backend.

---

## 4. Resource model (CPU / RAM / disk / time)

Sandboxing without budgets is a locked room with an infinite buffet. Resource limits are part of **task-scoped authority**.

### Three “sizes” (do not conflate)

| Kind | What grows | Primary control |
|------|------------|-----------------|
| **Image** | Tooling layers on disk | Image choice + pull policy; warn on first-run size |
| **Memory (RAM)** | Agent + children (npm, compilers, LSs) | Docker/cgroup `--memory` (strong) |
| **Workspace disk** | Ephemeral copy, `node_modules`, builds, logs | **Kotro must enforce** (Docker disk caps are weak/inconsistent for bind mounts) |

### Docker strengths vs gaps

| Resource | Docker | Gap / Kotro duty |
|----------|--------|------------------|
| RAM | Hard cap → OOM kill | Clear errors; permit field to raise; prefer no/low swap |
| CPU | Throttle via shares/`--cpus` | Optional timeout still needed |
| Disk (bind-mounted ephemeral dir) | Weak | Preflight free space; watch size; kill/stop before host/VM disk full |
| PIDs / fork bomb | `--pids-limit` | Set a default |
| Wall clock | Not automatic | TTL / `max_runtime` on permit |
| API $ cost | N/A in Docker | Kotro proxy budgets (when all LLM traffic forced through Kotro) |

### Mac / Docker Desktop outer ceiling

```text
Physical machine
  └── Docker Desktop VM (RAM + disk image in Desktop settings)
        └── Agent container (Kotro --memory / --cpus)
```

Effective limit = **min(permit caps, Desktop VM size, physical machine)**.  
Document this for users; preflight can warn when Desktop disk/RAM looks tight.

### When limits are hit (required behavior)

| Event | Product behavior |
|-------|------------------|
| Memory OOM | Fail run; record `resource_exhausted:memory`; **do not** retry unsandboxed |
| Disk quota / staging full | Stop writes or kill run; clear message; cleanup guidance |
| TTL exceeded | Kill container; export partial diff if safe/available |
| CPU throttle only | May continue slowly; combine with TTL |
| Sandbox unavailable | Refuse start (fail-closed) |

### Suggested alpha defaults (tunable; not sacred)

For a typical 16 GiB developer laptop (single run):

| Knob | Starting default | Notes |
|------|------------------|--------|
| Memory | 2–4 GiB hard, no/low swap | Large monorepos may need higher permit |
| CPUs | 2–3 | Leave headroom for IDE/UI |
| Workspace disk | 8–16 GiB Kotro-enforced | Refuse copy if repo + headroom exceeds |
| Runtime TTL | 30–60 min | Hard stop |
| Parallel Permit runs | 1 default | Avoid fighting Desktop VM |
| `pids-limit` | Set (e.g. hundreds, not unlimited) | Mitigate fork bombs |

PermitSpec / envelope should eventually carry resource budgets so the badge includes **how heavy** the worker may be — not only which paths/network.

### Ephemeral copy disk policy (Option A) — implementer checklist

1. Measure/estimate source repo size before copy.  
2. Refuse if copy + headroom exceeds `max_disk_workspace` or free space.  
3. Copy to staging outside the live tree.  
4. Mount staging into container (not the live checkout).  
5. During run: periodically check staging size; enforce cap.  
6. After run: produce patch/diff; delete or retain staging per documented policy.  
7. Never mount real `~/.ssh` or host secret dirs “for convenience.”

---

## 5. Implementation checklist (must consider)

Use during R0.1b contract, R2 `run --permit`, and docs/CLI work.

### Confinement

- [ ] Mandatory sandbox backend; refuse if missing  
- [ ] Agent not given `docker.sock` / podman socket  
- [ ] Agent cannot reach Kotro **control plane** (dashboard/admin)  
- [ ] Agent network `internal: true` as **public egress baseline**; host canary + gateway tests before claiming sole window  
- [ ] Dual-homed: agent → Kotro data-plane → upstream only  
- [ ] Provider/GitHub tokens only in Kotro/broker — `KOTRO_RUN_TOKEN` may enter agent  
- [ ] Option A inclusion policy; land via apply (R2-A) and/or clean-git draft PR (R2-B)  
- [ ] Narrow symlink semantics documented in code comments + user docs  
- [ ] Secret-read tests record **failure mode** (ENOENT/`FileNotFoundError` preferred)  
- [ ] Proxy death → fail closed (no direct-provider fallback from agent)

### Resources

- [ ] Apply memory + CPU + pids limits on container start  
- [ ] Permit/CLI knobs (or config) for those limits  
- [ ] Workspace disk preflight + runtime watch + hard fail  
- [ ] Runtime TTL  
- [ ] Explicit `resource_exhausted:*` errors in CLI/logs  
- [ ] Preflight Docker Desktop / free space warnings where detectable  
- [ ] Image pull size / first-run time called out in docs

### Product honesty

- [x] Pluggable backend interface (Docker first)  
- [x] No README/CLI language claiming hypervisor isolation on Docker tier  
- [ ] `--live-workspace` (if added) labeled lower security  
- [x] Spike/demo labeled **Containment feasibility spike** until permits/receipts exist  
- [x] User-facing “Limits & trust boundaries” section shipped with alpha (`docs/launch/PERMIT-LIMITS.md`)

### Acceptance mapping

See suite **#1–#24** (+ #25–#27 Sol adds) in [`KOTRO-PERMIT-TASKS.md`](./KOTRO-PERMIT-TASKS.md).  
Resource exhaustion is additional operational behavior — add Escape Lab / ops tests when implementing caps.

---

## 6. Required user-facing disclosures

Ship these (or equivalent) in: CLI `--help` / first-run, README Permit section, and a short **Limits & trust boundaries** page.

### 6.1 What Permit does

- Runs the agent inside a **sandbox** (alpha: Docker container) for each `run --permit`.  
- Grants only what the **permit** allows for a **short time**.  
- Keeps work on an **ephemeral copy** with an explicit inclusion preview.  
- **R2-A:** you review and **apply**.  
- **R2-B:** you allow-once; **Kotro opens a draft PR** from a host-owned clean repo (GitHub token stays on the host; live path needs host `GITHUB_TOKEN`).  
- You **merge** on GitHub.  
- Sends model traffic through **Kotro**; the agent should not hold your provider API key.  
- Blocks direct internet from the agent (deny-all); outbound LLM/GitHub land calls go via Kotro.

### 6.2 What Permit does not do (Docker / alpha tier)

- It is **not** complete kernel-level / hypervisor isolation. Containers share a kernel (see Mac Desktop note).  
- It does **not** make every path inside the **container image** secret. Image contents are trusted execution material.  
- It does **not** protect secrets you **voluntarily mount** or paste into the workspace copy.  
- It does **not** replace your review / allow-once, and it does **not** merge for you.  
- It does **not** update Confluence/Figma in alpha.  
- Hardened broker features (attenuation, signed land receipts) shipped in R3 — claim only with [`../launch/PERMIT-ALPHA-CLAIMS.md`](../launch/PERMIT-ALPHA-CLAIMS.md).

### 6.3 Requirements to run

- **Docker Desktop ≥ 4.x for your Mac’s native arch** (Apple Silicon = arm64 build). An old Intel/HyperKit leftover on arm64 fatals at startup and looks like “Kotro doesn’t work.”  
- Docker Engine or Docker Desktop must be **running** (`docker info` shows a Server).  
- Enough **disk** for: image + ephemeral copy + headroom.  
- On Docker Desktop: configure VM **RAM/disk** large enough for your permit limits and project.  
- If sandbox cannot start, Kotro **will not** run the agent on the host as a fallback.

### 6.4 Resource behavior (tell users explicitly)

- Default **memory/CPU/time/disk** caps apply; raise them in the permit/config if legitimate builds need more.  
- Hitting the memory cap typically **kills** the run (OOM) — this is intentional protection for your machine.  
- Large `npm install` / compiles may need higher limits; that is expected, not a mysterious failure.  
- Disk growth in the ephemeral workspace is limited by Kotro policy; filling limits **fails the run**.  
- Only one heavy Permit run by default is recommended on laptops.

### 6.5 Suggested CLI / first-run blurb (draft copy)

```text
Kotro Permit runs your agent inside a Docker sandbox under a short-lived permit.
Your live repo and host secrets are not mounted. In the thesis path you review and apply
a diff. When the draft-PR broker (R2-B) is available, Kotro can open a draft PR after you
confirm — the agent never holds your GitHub token. You merge.

This is workspace and network confinement, not hypervisor-grade isolation.
Requires Docker Desktop ≥ 4.x (native arch) or Docker Engine; if unavailable, Permit
refuses to start (no host fallback).
internal: true blocks public routing but is not by itself “Kotro-only” — see docs.
Resource caps (memory/CPU/disk/time) apply — see docs for defaults and how to raise them.
```

### 6.6 Suggested README subsection titles

- **Permit: task-scoped authority**  
- **How the sandbox works (Docker)**  
- **Trust boundaries & non-goals**  
- **Resource limits**  
- **Mac / Docker Desktop notes**  
- **Troubleshooting:** OOM, disk full, Docker not running, permission vs not-found denials  

---

## 7. Threat vs control (quick matrix)

| Attacker / failure | Docker + Option A + deny-all + mediator | Still open / weaker |
|--------------------|----------------------------------------|---------------------|
| Read unmounted `~/.ssh` via shell/Python | Stopped (not present) | If operator mounts secrets into workspace |
| Direct exfil to internet | Stopped | Bugs in dual-home wiring; proxy compromise |
| Modify live host repo / hooks | Stopped (ephemeral copy) | `--live-workspace`; user applies malicious patch blindly |
| Steal API / GitHub key from agent env | Stopped if never injected | Misconfiguration injecting keys |
| Forged broker call / bait-and-switch PR | Mitigated by run token + artifact hash + allow-once | Missing L1–L4 checks |
| Fill disk / OOM laptop | Mitigated if caps enforced | Caps unset; Desktop VM disk undersized |
| Container escape / kernel exploit | **Not** a Docker-tier promise | Needs microVM/VM strong tier |
| Malicious content *inside* allowed project copy | Agent can still damage the **copy** | Human review of patch remains mandatory |

---

## 8. Landing work: patch → draft PR → merge (keep it simple)

**Design goal:** Security in the sandbox; landing feels like modern AI (**Kotro opens draft PR**), not a manual git homework assignment.

```text
  RUN (agent in Docker)  →  REVIEW / allow-once (you)  →  DRAFT PR (Kotro broker)  →  MERGE (you)
```

Full protocol: [`KOTRO-PERMIT-BROKER.md`](./KOTRO-PERMIT-BROKER.md).

The agent **never** merges to main. Merge stays human always.

### 8.1 What the container does *not* do

| Action | In agent container? |
|--------|---------------------|
| Edit files in ephemeral copy | Yes |
| Call Kotro broker with `KOTRO_RUN_TOKEN` | Yes (intent only) |
| Hold `GITHUB_TOKEN` / `gh pr create` to GitHub | **No** |
| Merge PR | **No** |
| Update Confluence / Figma / external docs SaaS | **No** (alpha) |

### 8.2 Alpha happy path (R2 + thin broker)

```text
1. kotro run --permit … -- <agent>
2. Agent edits ephemeral copy
3. Agent (or end-of-run UX) requests draft PR via Kotro
4. YOU allow-once (confirm artifact/diff)
5. Kotro host broker pushes + opens draft PR → returns URL
6. YOU merge on GitHub
```

Fallback if GitHub not configured: show diff + `kotro apply` only — still fail closed on token-in-agent.

| Actor | Role |
|-------|------|
| Agent (container) | Produce changes; request land via run token |
| You | Allow-once / review |
| Kotro broker (host) | Hold GitHub creds; create **draft** PR |
| You (GitHub) | **Merge** |

### 8.3 Handshake (summary)

- **Not required:** exotic multi-round crypto; mTLS day one; GitHub OAuth inside agent.
- **Required:** mint **run-scoped token** at start; validate on broker call; allow-once; artifact hash bind.
- Details and API sketch: broker doc §§4–7.

### 8.4 Commit & merge

| Step | Who |
|------|-----|
| Write code/docs | Agent (copy) |
| Allow-once / approve artifact | **You** |
| Push + draft PR | **Kotro broker (host)** |
| Merge | **You** |

Default permit: `draft_pr` max — never `merge`.

### 8.5 Crucial workflows

| Workflow | Alpha |
|----------|--------|
| In-repo docs / code / design-as-files | **Primary** — brokered draft PR |
| Confluence / Notion / Figma | **Out of scope** — later broker or human |

### 8.6 Simplicity bar

≤ 3 user-visible steps: **Run → Allow-once / review → Merge**.
Containers, run tokens, brokers stay internal.

### 8.7 Anti-patterns

- `GITHUB_TOKEN` in container
- Auto-merge from agent
- Broker without run token or artifact bind
- Silent push without allow-once (alpha default)
- Claiming Confluence/Figma under Permit in alpha
- Starting broker work before R0.1a passes

### 8.8 Implementer UX checklist

- [ ] Single review artifact + allow-once
- [ ] Draft PR URL is the obvious success output
- [ ] Merge never in default permit
- [ ] Docs: positioning sentence from broker doc
- [ ] Confluence/external design = out of scope for alpha

---

## 9. Future movement (do not build all at once)

1. **R0.1a** — Containment spike; **hard stop if fail**; no broker coding before pass.
2. **R2.1–R2.3** — `run --permit` + Option A + dual-home + disclosures.
3. **R2.4–R2.5** — Thin broker + run token + allow-once draft PR.
4. **Gate B** — real users complete runs (with PRs).
5. **R3** — Harden broker (attenuation, receipts); Escape Lab.
6. **Later** — MicroVM tier; resource budgets; `--live-workspace`; Confluence brokers; real egress firewall.

---

## 10. Document history

| Date | Change |
|------|--------|
| 2026-08-06 | Initial: sandbox vs permit, Docker lifecycle, VM tradeoff, resources, impl checklist, user disclosures |
| 2026-08-06 | Added §8 landing flow: patch/PR/merge, docs vs Confluence/design, simplicity bar |
| 2026-08-06 | Aligned §8 with v7: thin broker in alpha; run-token handshake; link broker doc |
| 2026-08-06 | Sol: qualify internal:true; R2-A apply vs R2-B PR; suite #1–#24; inclusion policy |
