# R2-A / R2.3b dogfood

**Purpose:** Gate B *partial* evidence — containment + dual-home dataplane + apply land.  
**Not:** broker / draft-PR demo (that is R2-B).

```bash
./spikes/r2a-dogfood/run.sh
```

Proves:

1. Unit gates (run token mint, R2-A completed path, staging)
2. Agent on `--internal` net reaches dual-homed data-plane
3. `PROVIDER_TOKEN` / provider keys absent from agent env; `KOTRO_RUN_TOKEN` present
4. Reviewed unified diff applies to the live host repo via `kotro-proxy apply`
