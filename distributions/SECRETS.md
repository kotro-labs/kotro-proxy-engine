# Distribution registry secrets

Configure these in **GitHub → Settings → Secrets and variables → Actions** on `ramairwing/kotro-proxy-engine`.

| Secret | Purpose | How to obtain |
|--------|---------|---------------|
| `NPM_TOKEN` | Publish `@kortosystems/proxy-engine` on tag push | [npmjs.com](https://www.npmjs.com) → Access Tokens → **Automation** token |
| `VSCE_PAT` | Publish `kortosystems.kortolabs-proxy-engine` to VS Code Marketplace | [Azure DevOps PAT](https://dev.azure.com/_users/settings/tokens) with **Marketplace → Manage** scope |

**Publisher:** `kortosystems` — [Manage publisher](https://marketplace.visualstudio.com/manage/publishers/kortosystems)

## VSCE_PAT (Marketplace publish token)

The URL `dev.azure.com/_users/settings/tokens` often returns **404**. Use one of these paths instead:

### Option A — From Marketplace (easiest)

1. Stay on [marketplace.visualstudio.com/manage/publishers/kortosystems](https://marketplace.visualstudio.com/manage/publishers/kortosystems)
2. Click your **profile / name** (top right, near Sign out)
3. Look for **Personal access tokens** or a link to **Azure DevOps**
4. Create token with:
   - **Organization:** **All accessible organizations** (required)
   - **Scopes:** Custom → **Show all scopes** → **Marketplace → Manage**

### Option B — Via Azure DevOps (if Option A doesn’t show tokens)

1. Open [https://dev.azure.com](https://dev.azure.com) and sign in with `prameshchennai@gmail.com`
2. If prompted, **create a free organization** (any name, e.g. `kortosystems-dev`)
3. Click your **profile icon** (top right) → **Personal access tokens**  
   Or go directly to:  
   `https://dev.azure.com/{YourOrgName}/_usersSettings/tokens`  
   (replace `{YourOrgName}` with the org you created — note `_usersSettings`, not `_users/settings`)
4. **+ New Token** → same scopes as above (**Marketplace → Manage**, **All accessible organizations**)
5. Copy token → GitHub secret **`VSCE_PAT`**

### Option C — Verify with terminal (optional)

```bash
cd distributions/vscode-extension
npx @vscode/vsce login kortosystems
# Paste the PAT when prompted — should print "verification succeeded"
```

**Do not** use the manual **Upload extension** dialog on Marketplace — CI publishes via `vsce publish`.

Close the upload modal; you don't need to drag a `.vsix` there.

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
