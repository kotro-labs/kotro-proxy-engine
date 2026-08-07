# Kotro Permit — Limits & trust boundaries

User-facing disclosures for alpha (Docker tier). Keep this language in CLI help, README, and demos.

## What Permit does

- Runs the agent inside a **Docker sandbox** for each `kotro-proxy run --permit`.
- Grants only what the **signed permit** allows for a **short time** (`[not_before, expires_at)`).
- Stages an **ephemeral copy** of allowed repo content (Option A — not a live RW mount of your tree).
- **R2-A:** you review `*.review.diff` and run `kotro-proxy apply`.
- **R2-B:** after allow-once, Kotro can open a **draft PR** from a **host-owned clean git** repo (GitHub token stays on the host).
- You **merge** on GitHub.
- Model traffic goes through Kotro’s dataplane when dual-home is enabled; the agent should not hold your provider API key.
- Agent outbound to the public internet is denied by default (`--internal` agent net); LLM/broker calls go via Kotro.

## What Permit does not do (Docker / alpha)

- It is **not** complete kernel-level or hypervisor isolation. Containers share a kernel.
- It does **not** make every path inside the **container image** secret. The image is trusted execution material.
- It does **not** protect secrets you **voluntarily** put into the staged workspace.
- It does **not** replace your review / allow-once, and it does **not** merge for you.
- It does **not** update Confluence / Notion / Figma in alpha.
- `internal: true` blocks **public** routing; it is **not** by itself proof of “only Kotro can talk to the agent.”
- Opening a **draft** PR may still trigger **CI, bots, and `pull_request` apps**.

## Requirements

- **Docker Desktop ≥ 4.x for your machine’s native arch** (Apple Silicon = arm64). Old Intel/HyperKit leftovers fail at startup and look like “Kotro doesn’t work.”
- Docker must be **running** (`docker info` shows a Server).
- Enough disk for image + ephemeral copy + headroom.
- If the sandbox cannot start, Kotro **will not** run the agent on the host as a fallback.

## Resource behavior

- Default memory / CPU / pids caps apply on the agent container.
- Hitting the memory cap typically **kills** the run (OOM) — intentional.
- Large installs/compiles may need higher limits.
- Prefer one heavy Permit run at a time on a laptop.

## First-run blurb (copy/paste)

```text
Kotro Permit runs your agent inside a Docker sandbox under a short-lived signed permit.
Your live repo and host secrets are not mounted. Review and apply a diff, or confirm a
Kotro-brokered draft PR — the agent never holds your GitHub or provider tokens. You merge.

This is workspace and network confinement, not hypervisor-grade isolation.
Requires Docker Desktop ≥ 4.x (native arch) or Docker Engine; if unavailable, Permit
refuses to start (no host fallback). Draft ≠ no CI. See docs/launch/PERMIT-ALPHA-CLAIMS.md.
```

## Related

- Honest claim matrix: [`PERMIT-ALPHA-CLAIMS.md`](./PERMIT-ALPHA-CLAIMS.md)
- Design pack: [`../roadmap/KOTRO-PERMIT-README.md`](../roadmap/KOTRO-PERMIT-README.md)
