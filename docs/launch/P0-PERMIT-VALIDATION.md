# P0 — Async validation (no cold calls required)

**Goal:** Get ≥3 written affirmations of the **remaining wedge** (not “sandbox sounds cool”) before more Permit engineering.  
**You do not need Zoom, DMs to strangers, or in-person talks.**  
**Canonical ask:** [`GATE-A-RECRUITING.md`](./GATE-A-RECRUITING.md) (Sol 2026-08-07 — containment is not the lead).

---

## Step-by-step (do in order)

### Step 1 — Practice the pitch once (5 min, alone)

Say out loud (timer 60s):

> Nono, srt or Docker enforce the boundary. Kotro makes the job authority
> portable, delegable and provable — CI-issued badges, attenuated sub-agents,
> source-bound grants, offline receipts across sandbox → land.

---

### Step 2 — Post the ask on X (today)

Reply to your existing Kotro tweet  
https://x.com/RameshPandian04/status/2085029921785799155  

**Paste (from Gate A — edit lightly in your own words if you want):**

```
Quick question if you already sandbox Claude Code / Cursor agents:

Containment is getting commodity (vendor sandboxes, Docker, Nono, …).
We're testing whether teams still need a portable job badge on top:

  • CI-issued signed task authority
  • attenuated delegation to sub-agents
  • grants bound to repo + base SHA
  • offline-verifiable receipts across sandbox → mediation → land

Do any of those still hurt for you? If three teams say yes, we'll
dogfood Kotro Permit on a real repo. If not, that's useful too.
```

**Done when:** reply is live.

---

### Step 3 — Same ask on Dev.to (today)

Open your post:  
https://dev.to/rameshpandian/kotro-a-local-control-plane-for-coding-agents-mcp-llm-one-rust-binary-26he  

Add a **comment** (not a new article):

```
Follow-up: next focus is task-scoped permits (limit what a manipulated
agent can do for one task — not another cost/firewall proxy).

If you run coding agents with MCP/tools: would you try a <3 min permit
path on a real task? yes / no / maybe — and why.
```

**Done when:** comment is published.

---

### Step 4 — One community post (optional, tomorrow — pick ONE)

Choose **only one**:

| Place | How |
|--------|-----|
| A Discord you already use (Claude / MCP / Continue) | Short message in an appropriate channel |
| r/ClaudeAI or r/LocalLLaMA | Text post — more prose than link dump; use flair |

**Paste:**

```
Title: Would you run a coding agent under a short-lived task permit?

Body:
I maintain Kotro (local agent control plane). I’m validating a narrower idea:

Sign a permit for one task — filesystem, network, MCP tools, budget.
Assume the model can be manipulated; prevent actions outside the permit.

Would you try this on a real Claude Code / Codex task if setup were <3 minutes?
yes / no / maybe + why.

Not selling a call — just written feedback. Repo: github.com/kotro-labs/kotro-proxy-engine
```

If Reddit filters remove it, **don’t** repost the same day. Rely on X + Dev.to.

**Done when:** one post is up, or you skip this step consciously.

---

### Step 5 — Wait and collect (3–7 days)

- Check X / Dev.to once a day  
- Reply briefly to anyone who answers (thank them; ask one clarifying question if useful)  
- **Do not** email HN again, resubmit Show HN, or build `kotro run` yet  

Fill the tracker below.

---

### Step 6 — Score answers

| Reply type | Count as |
|------------|----------|
| “Yes” / “I’d try on a real task” | **Hard yes** |
| “Maybe if …” | Maybe (note the condition) |
| “No” + reason | No (useful learning) |
| Likes with no text | **Ignore** (not a yes) |

---

### Step 7 — Gate decision

| Result | Next |
|--------|------|
| **≥3 hard yes** | Start **P1** (Permit-first README) |
| **1–2 hard yes** or only maybes | One more async round (different community); don’t build full MVP |
| **0 hard yes** after 7 days | Stay on v0.6.3 + EL-08; **do not** build Permit CLI yet |

---

## Tracker (fill in)

| Date | Channel | Hard yes | Maybe | No | Notes |
|------|---------|----------|-------|-----|-------|
| | X reply | | | | |
| | Dev.to comment | | | | |
| | Other (optional) | | | | |
| **Totals** | | | | | |

- Hard yes total: __  
- Decision: proceed P1 / more listening / stop Permit  
- Date decided: __  

---

## What you are validating

People want: **task-scoped authority** (permit), not “another LLM proxy.”

One-line demo in words (no code required for P0):

> Poisoned README tells agent to read ~/.ssh → permit denies → receipt shows deny.

Be honest if asked: full FS/network broker is roadmap; you’re asking if they’d use it when ready.

**Containment evidence (use when people ask “does it actually block?”):**  
R0.1a PASS — `spikes/r0.1a-containment/results/PASS-20260807.md`  
Recruiting playbook + clip storyboard: [`GATE-A-RECRUITING.md`](./GATE-A-RECRUITING.md)

---

## Explicit non-goals during P0

- Cold DMs to strangers  
- Zoom interviews (optional later if someone offers)  
- Implementing `kotro run`  
- Another Show HN  
- Updating HN flagged comment  
