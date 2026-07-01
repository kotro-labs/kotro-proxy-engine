# Distribution registry secrets

Configure these in **GitHub → Settings → Secrets and variables → Actions** on `ramairwing/kotro-proxy-engine`.

| Secret | Purpose | How to obtain |
|--------|---------|---------------|
| `NPM_TOKEN` | Publish `@kortosystems/proxy-engine` on tag push | [npmjs.com](https://www.npmjs.com) → Access Tokens → **Automation** token |
| `VSCE_PAT` | Publish `kortosystems.kortolabs-proxy-engine` to VS Code Marketplace | [Azure DevOps PAT](https://dev.azure.com/_users/settings/tokens) with **Marketplace → Manage** scope |

**Publisher:** `kortosystems` — [Manage publisher](https://marketplace.visualstudio.com/manage/publishers/kortosystems)

## VSCE_PAT (you are NOT on the right screen for this)

The **Upload extension** dialog in Marketplace is for manual `.vsix` uploads. CI uses `vsce publish` instead — close that modal.

Create the token here:

1. Open [https://dev.azure.com/_users/settings/tokens](https://dev.azure.com/_users/settings/tokens)  
   (Same Microsoft account as Marketplace: `prameshchennai@gmail.com`)
2. **+ New Token**
3. Scopes: **Custom defined** → **Marketplace** → check **Manage**
4. Copy token → GitHub secret named exactly **`VSCE_PAT`**

## NPM_TOKEN

1. Create npm org **kortosystems** at [npmjs.com/org/create](https://www.npmjs.com/org/create)  
   *Or use `@ramairwing/proxy-engine` if you prefer your personal scope — update `distributions/npm-cli/package.json`.*
2. **Access Tokens** → **Automation** token with publish access
3. GitHub secret named exactly **`NPM_TOKEN`**

## Go-live sequence (first public release)

**Do not re-dispatch the tag until both secrets are active.**

### 0. Add secrets

[Repository secrets dashboard](https://github.com/ramairwing/kotro-proxy-engine/settings/secrets/actions)

### 1. Re-dispatch tag (after secrets are live)

```bash
make go-live VERSION=v0.1.0
```

### 2. Monitor CI

https://github.com/ramairwing/kotro-proxy-engine/actions/workflows/release.yml

### 3. Stamp Homebrew checksums (after release assets upload)

```bash
make post-release-homebrew VERSION=v0.1.0
git push origin main
```

### 4. Verify public telemetry

| Surface | URL |
|---------|-----|
| GitHub Release | https://github.com/ramairwing/kotro-proxy-engine/releases |
| npm | https://www.npmjs.com/package/@kortosystems/proxy-engine |
| VS Code Marketplace | https://marketplace.visualstudio.com/items?itemName=kortosystems.kortolabs-proxy-engine |

If secrets are absent, the release workflow **skips** registry publish and still uploads GitHub Release assets + `.vsix`.

## Homebrew tap

```bash
scripts/update-homebrew-shas.sh v0.1.0
```

Sync `distributions/homebrew-tap/Formula/` into `github.com/ramairwing/homebrew-tap`.
