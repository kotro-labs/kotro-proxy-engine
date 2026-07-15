# Viral repo packaging → Kotro

Reference list (Jul 2026 “went viral” roundup): Meetily, agent-skills, system_prompts_leaks, OfficeCLI, Orca, OmniRoute, Claude Video Reader, page-agent, Astryx.

## What those repos share

| Pattern | Examples | Kotro equivalent |
|---------|----------|------------------|
| One-sentence miracle claim | “Give Claude ability to watch video”, “Never stop coding” | Local firewall + bill cut for Cursor/Claude Code |
| Demo in viewport 1 | GIFs / poster→video | Injection MP4 + dashboard PNG |
| Install in first ~20 lines | `npx`, `curl`, `<script>` | `curl \| bash` / brew / Marketplace |
| Pain → fix table | OmniRoute, OfficeCLI | Without / with Kotro |
| Named audiences | Cursor, Claude Code, “for agents” | Cursor · Claude Code · OpenAI-compatible |
| Concrete numbers | 250 providers, 15–95% | **68%** savings · **2** injections blocked · **~15MB** |
| Parallel install CTAs | Agents vs humans | brew / curl / extension |
| Honest constraint | “local / no cloud” | “HTTP path, not raw MCP stdio” |

## Structural rule

Viral peers invert the academic README:

**claim → demo → who → one-liner → before/after → then nuance**

Kotro previously led with comparison matrix + 99.3% footnote *before* install. That kills share momentum.

## What not to copy

- 40-language README spam
- Misleading headline numbers (keep 99.3% demoted + caveated)
- “Best forever” claim tables that invite HN pile-ons

## Executed

- README first screen reshaped to the viral outline (see root `README.md`).
- Launch assets linked: `docs/launch/assets/`, `make demo-injection`, `make demo-savings`.
- Port consistency: prefer `:8080` / `:9090` (not stale `:3000` in Cursor guide).
