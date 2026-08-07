# R0.1a — Containment feasibility spike

**Label:** Containment feasibility spike  
**Not:** a Permit / signature / receipt demo.

## What this proves

| # | Claim |
|---|--------|
| 4 | Shell cannot read host SSH key (prefer ENOENT) |
| 5 | Python cannot read host SSH key (prefer `FileNotFoundError`) |
| 6 | **External DNS and HTTP** both fail as channels (Sol P0 — DNS success = FAIL) |
| 7 | Hostile-IP connect denied |

Host secret in `host-secrets/` is **never** bind-mounted. Agent sees only `workspace/`.

`internal: true` alone is **not** “Kotro-only” (host/gateway reachability = R0.1b).

## Run

```bash
# Docker Desktop must be running
./run.sh
```

Results → `results/`.  
**Do not** treat logs from before the DNS-assert fix as Gate A evidence.

## Failure modes

- `FileNotFoundError` / ENOENT → not mounted (**wanted**)  
- `PermissionError` / EACCES → over-mount (**investigate**)  
- Secret bytes in stdout → **hard fail**  
- `DNS_EXT_OK` / `DNS_OK` / `HTTP_OK` → **#6 fail**
