# VS Code Marketplace — manual publish (no VSCE_PAT)

Use this when **Azure DevOps blocks PAT creation** (subscription required, 404 on token page, etc.).  
No `VSCE_PAT`, no `vsce login`, no separate git repo.

**Publisher:** [kortosystems](https://marketplace.visualstudio.com/manage/publishers/kortosystems)  
**Extension ID:** `kortosystems.kortolabs-proxy-engine`

---

## Per-release checklist (~2 minutes)

### 1. Wait for GitHub Release

After you push a tag, CI builds and attaches the VSIX:

https://github.com/kotro-labs/kotro-proxy-engine/releases

Example: [v0.2.7](https://github.com/kotro-labs/kotro-proxy-engine/releases/tag/v0.2.7) → download **`kortolabs-proxy-engine.vsix`**

### 2. Upload on Marketplace

1. Open [Manage publisher → kortosystems](https://marketplace.visualstudio.com/manage/publishers/kortosystems)
2. Click **KortoLabs Proxy Engine**
3. Click **⋮** (or **Update**) → **Upload new version** / **Update**
4. Drag **`kortolabs-proxy-engine.vsix`** from the GitHub Release
5. Confirm — version must match `package.json` inside the VSIX (e.g. `0.2.7`)

### 3. Verify

- Overview: [Marketplace listing](https://marketplace.visualstudio.com/items?itemName=kortosystems.kortolabs-proxy-engine)
- Version number matches the tag
- Screenshots load (README uses absolute GitHub raw URLs)

---

## What still automates without VSCE_PAT

| Channel | On `git push tag v*` |
|---------|----------------------|
| GitHub Release + VSIX | ✅ CI |
| npm `@kortosystems/proxy-engine` | ✅ CI (needs `NPM_TOKEN`) |
| Homebrew tap | ✅ script after release |
| **VS Code Marketplace** | ❌ **manual upload** (this doc) |

The `marketplace-publish.yml` workflow will fail until `VSCE_PAT` is added — that is expected. Ignore it or disable notifications for that workflow.

---

## Common issues

| Problem | Fix |
|---------|-----|
| "Version already exists" | You already uploaded that semver; only upload once per tag |
| Extension not listed | First time: use **+ New extension** instead of Update |
| Broken README images | Use `raw.githubusercontent.com/.../distributions/vscode-extension/media/` URLs |
| Wrong binary in VSIX | Always use VSIX from **GitHub Release**, not a local repack |

---

## Optional: unblock VSCE_PAT later

If you ever want full automation:

1. Marketplace profile → Personal access tokens (or Azure DevOps with billing card)
2. Scope: **Marketplace → Manage**
3. GitHub secret: `VSCE_PAT`
4. Re-run [Publish VS Code Marketplace](https://github.com/kotro-labs/kotro-proxy-engine/actions/workflows/marketplace-publish.yml) for `v0.2.7`

Until then, manual upload after each release is **official and fine** — many publishers do this.
