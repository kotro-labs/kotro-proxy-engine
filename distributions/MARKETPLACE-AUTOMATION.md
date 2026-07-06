# VS Code Marketplace automation

> **Stuck on `VSCE_PAT`?** Skip automation — use **[MARKETPLACE-MANUAL.md](MARKETPLACE-MANUAL.md)** (download VSIX from GitHub Release → upload on publisher page). That is the supported path when Azure blocks token creation.

Manual VSIX upload is **not required** once `VSCE_PAT` is configured in this repository.

## Recommended setup (same repo — no second git repo)

A separate repository is usually **unnecessary**. Marketplace publish only needs one secret and two workflows that already live here:

| Workflow | When it runs |
|----------|----------------|
| [`release.yml`](../.github/workflows/release.yml) | Tag push `v*` → builds VSIX, npm, GitHub Release |
| [`marketplace-publish.yml`](../.github/workflows/marketplace-publish.yml) | **Automatically** on `release: published` → uploads VSIX to Marketplace |

### One-time setup (~5 minutes)

1. Create a **Personal Access Token** with **Marketplace → Manage** scope  
   (see [SECRETS.md](SECRETS.md) → VSCE_PAT section)

2. Add GitHub secret **`VSCE_PAT`**  
   [Repository secrets](https://github.com/ramairwing/kotro-proxy-engine/settings/secrets/actions) → **New repository secret**

3. Verify locally (optional):

   ```bash
   cd distributions/vscode-extension
   npx @vscode/vsce login kortosystems
   ```

### What happens on every release

```
git tag v0.2.8 && git push origin v0.2.8
        │
        ▼
  release.yml (build + npm + GitHub Release with VSIX)
        │
        ▼
  release:published event
        │
        ▼
  marketplace-publish.yml (vsce publish --packagePath VSIX)
        │
        ▼
  Marketplace shows new version (usually within 1–2 minutes)
```

The Marketplace workflow publishes the **exact VSIX** attached to the GitHub Release — same artifact users can download manually.

### Republish without re-tagging

If a release already exists but Marketplace was skipped (e.g. before `VSCE_PAT` was added):

1. Open [Actions → Publish VS Code Marketplace](https://github.com/ramairwing/kotro-proxy-engine/actions/workflows/marketplace-publish.yml)
2. **Run workflow** → enter tag `v0.2.7` (or leave empty for latest release)
3. Workflow downloads that release's VSIX and publishes

## Why not a separate git repo?

| Approach | Pros | Cons |
|----------|------|------|
| **Same repo + `VSCE_PAT`** (recommended) | Simple; one release pipeline; republish via dispatch | Secret visible to repo admins |
| **Separate `marketplace-bot` repo** | Isolates Marketplace credentials | Extra repo, cross-repo triggers, duplicate CI, harder to debug |
| **Manual upload** | No Azure/PAT | Blocks every release; easy to forget |

Use a separate repo only if you need **strict credential isolation** (e.g. contractors with write access to engine code but not Marketplace). Pattern:

```
kotro-proxy-engine (tag push)
    → repository_dispatch → marketplace-bot repo
        → download VSIX from GitHub Release API
        → vsce publish
```

That adds complexity with little benefit for a solo/small-team publisher.

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| Workflow skipped / `VSCE_PAT not set` | Add secret; re-run **Publish VS Code Marketplace** for the tag |
| `already published version` | Marketplace already has that semver — bump `package.json` and cut a new tag |
| Azure PAT 404 | Use Marketplace profile → Personal access tokens ([SECRETS.md](SECRETS.md)) |
| Images broken on Marketplace | README must use absolute `raw.githubusercontent.com` URLs ([vscode README](vscode-extension/README.md)) |

## Related

- [SECRETS.md](SECRETS.md) — `VSCE_PAT` and `NPM_TOKEN`
- [Publisher dashboard](https://marketplace.visualstudio.com/manage/publishers/kortosystems)
