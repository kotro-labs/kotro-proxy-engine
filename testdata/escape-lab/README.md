# Escape Lab

A reproducible corpus of agent-security scenarios, and a runner that measures what
Kotro actually does about each one.

The point is not to show Kotro winning. Several scenarios are declared `none` —
no coverage today — with a stated reason and the phase that addresses them. The
run is green when a known gap behaves like a known gap. That makes this a
regression gate rather than a scorecard.

## Running it

```bash
# Structure only. No proxy needed, runs in seconds.
python3 scripts/escape-lab.py --validate

# What groups exist and what env each needs.
python3 scripts/escape-lab.py --list-groups

# Against a live proxy, one group at a time.
python3 scripts/escape-lab.py \
  --target http://127.0.0.1:8080 \
  --control-token "$KOTRO_CONTROL_TOKEN" \
  --env-group default \
  --out results.json \
  --markdown docs/security/ESCAPE-LAB-MATRIX.md
```

Exit codes: `0` all scenarios matched their declaration, `1` divergence or invalid
corpus, `2` no proxy reachable at `--target`.

## Env groups

Some scenarios need mutually exclusive proxy configuration — injection warn mode
and block mode cannot both be true in one process, and `KOTRO_MODE` is a
three-way dial (`disabled` | `audit` | `enforce`). Scenarios sharing a config
form an `env_group`, and the runner executes one group per invocation. CI runs a
job per group, starting the proxy with that group's environment.

`--list-groups` prints each group with the environment it expects. A scenario
with no special requirement belongs to `default`.

Every response carries `x-kotro-mode`. The runner records the observed mode in
results JSON and stamps it into the matrix header so published `prevent` /
`detect` rows stay reproducible. EL-12 (`mode-audit`) and EL-13
(`mode-disabled`) assert the dial itself.

## Harnesses

`harness: http` (the default) means the scenario is measurable by this runner.
`harness: cli` means enforcement lives outside the proxy's HTTP path — `mcp-wrap`
sits on MCP stdio — so the runner **skips** it rather than reporting a gap.

That distinction matters. EL-05 (tool rug pull) is a capability Kotro genuinely
has; it is simply not observable from here. Reporting it as `none` would
understate coverage, and posting a synthetic `tool_drift` event only to assert it
came back would prove nothing about the quarantine logic. Skipping is the honest
third option. Those scenarios need a CLI harness before they can be measured.

## Setup and teardown

Scenarios may declare `setup` steps that run before the attack and `teardown`
steps that run after it, pass or fail. The control token is attached
automatically.

Teardown is **mandatory whenever setup exists**, and the validator enforces it.
Setup usually mutates state that outlives the request — engaging the kill switch
in EL-06 persists until something disengages it — so a missing teardown would
leave every later scenario blocked and make results depend on execution order.

Without a control token the flight recorder is unreadable, so evidence columns
report as unverified and outcomes are derived from status and headers alone. Pass
the token for a complete run.

## Outcome vocabulary

| Outcome | Meaning |
|---|---|
| `prevent` | Blocked before the effect occurred |
| `transform` | Request allowed; harmful content removed (redaction) |
| `detect` | Allowed through, but flagged with evidence on the tape |
| `observe` | Surfaced by a reporting endpoint, no security verdict |
| `none` | No coverage today — a tracked gap with a stated reason |

A generic `request` or `cache_*` flight event is deliberately **not** counted as
coverage. Logging that something happened is not the same as governing it, and
counting it would make the matrix flatter to Kotro than the truth.

## Divergence is a failure in both directions

The runner fails when observed behaviour differs from the declaration either way.
A regression is obvious. An *improvement* also fails, on purpose: if a scenario
declared `none` starts being prevented, someone should update the corpus
deliberately and say so in the commit, rather than letting the matrix drift
upward unnoticed.

## Adding a scenario

1. Append to `scenarios.json` with the next free `EL-NN`. IDs are stable and are
   never reused or renumbered — published results reference them.
2. Declare the outcome you expect **today**, not the one you want.
3. If that outcome is `none`, `gap_reason` is required and must name either the
   compensating control or the phase that closes the gap.
4. Run `--validate`, then a live run.

`scenario.schema.json` is the normative shape. The runner additionally enforces
unique IDs and the `gap_reason` rule, which a generic JSON Schema validator
cannot express.

## Scope

Scenarios exercise the HTTP path Kotro sits on: the model plane and the MCP
action plane. An agent that shells out, opens a raw socket, or otherwise reaches
the network without transiting the proxy is outside what this corpus can
measure — see EL-09 and `docs/security/THREAT-MODEL.md`.
