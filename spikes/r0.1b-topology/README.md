# R0.1b — Topology + Option A staging

## Topology (#16 / #25)

```bash
chmod +x run.sh stage-repo.sh test-stage-safety.sh
./run.sh
```

**#25 (Sol rename):** *host canary content unreachable; gateway exposure recorded* — not “gateway reachability denied.”  
Gateway may still be L3-addressable (`Connection refused`); see results honesty note.

## Staging (hardened)

```bash
./stage-repo.sh --repo /path/to/repo --preview
./stage-repo.sh --repo /path/to/repo --rev HEAD
./stage-repo.sh --repo /path/to/repo --include-untracked notes/wip.md

# Safety tests (R0.3 precursor)
./test-stage-safety.sh
```

- Output only under `$KOTRO_STAGING_ROOT` (default `~/.kotro/staging`) via `mktemp -d`  
- No caller-controlled `rm -rf`  
- Rejects `../` and absolute extras; nested deny-list  
- Host-side `*.manifest.jsonl` with sha256 (not in agent tree)  
- Warns that tracked/committed sensitive names are still staged  
