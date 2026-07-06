# Distribution registry secrets

Configure these in **GitHub → Settings → Secrets and variables → Actions** on `ramairwing/kotro-proxy-engine`.

| Secret | Purpose | How to obtain |
|--------|---------|---------------|
| `NPM_TOKEN` | Publish `@kortosystems/proxy-engine` on tag push | [npmjs.com](https://www.npmjs.com) → Access Tokens → **Automation** token |
| `VSCE_PAT` | Publish `kortosystems.kortolabs-proxy-engine` to VS Code Marketplace | [Azure DevOps PAT](https://dev.azure.com/_users/settings/tokens) with **Marketplace → Manage** scope |

**Automated Marketplace publish:** optional — requires `VSCE_PAT` (often blocked by Azure).  
**If stuck on PAT:** use [MARKETPLACE-MANUAL.md](MARKETPLACE-MANUAL.md) (~2 min per release, no Azure).

## VSCE_PAT — when you need it (and when you don't)

| Goal | VSCE_PAT required? |
|------|-------------------|
| **Manual upload** of `.vsix` on Marketplace web UI | **No** |
| **`vsce publish`** in CI or terminal | **Yes** |
| **`vsce login`** to verify publisher | **Yes** |

**Fastest path without Azure subscription:** build `.vsix` locally and drag it into  
[marketplace.visualstudio.com/manage/publishers/kortosystems](https://marketplace.visualstudio.com/manage/publishers/kortosystems)  
→ **New extension** → upload file. Public install counter starts immediately.

```bash
# After CI completes, download kortolabs-proxy-engine.vsix from GitHub Releases
# Upload via Marketplace → kortosystems → New extension
```

Only pursue `VSCE_PAT` below if you want **fully automated** `vsce publish` in GitHub Actions.

## VSCE_PAT (automated CI publish only)

The URL `dev.azure.com/_users/settings/tokens` often returns **404**. Use one of these paths instead:

### Option A — From Marketplace (easiest)

1. Stay on [marketplace.visualstudio.com/manage/publishers/kortosystems](https://marketplace.visualstudio.com/manage/publishers/kortosystems)
2. Click your **profile / name** (top right, near Sign out)
3. Look for **Personal access tokens** or a link to **Azure DevOps**
4. Create token with:
   - **Organization:** **All accessible organizations** (required)
   - **Scopes:** Custom → **Show all scopes** → **Marketplace → Manage**

### Option B — Via Azure DevOps (blocked if subscription required)

Microsoft may require linking **Pay-As-You-Go** billing before org creation. Azure Free Trial accounts often fail here.

| If you see "We couldn't find any subscriptions" | What to do |
|------------------------------------------------|------------|
| Want Marketplace **now** | Use **manual `.vsix` upload** above (no PAT) |
| Want **CI automation** | Click **Get started with Azure** → Pay-As-You-Go (PAT creation is free; card verification only) |
| Avoid Azure billing | Use a **fresh Outlook account** never used for Azure, or defer automated publish |

If org creation succeeds:

1. Profile icon (top right) → **Personal access tokens**  
   Or: `https://dev.azure.com/{YourOrgName}/_usersSettings/tokens`
2. **+ New Token** → **All accessible organizations** + **Marketplace → Manage**
3. GitHub secret **`VSCE_PAT`**

### Option C — Verify with terminal (optional)

```bash
cd distributions/vscode-extension
npx @vscode/vsce login kortosystems
# Paste the PAT when prompted — should print "verification succeeded"
```

**Manual upload** (publisher owner, no PAT): use **New extension** on  
[marketplace.visualstudio.com/manage/publishers/kortosystems](https://marketplace.visualstudio.com/manage/publishers/kortosystems)  
and upload `kortolabs-proxy-engine.vsix` from GitHub Releases.

## NPM_TOKEN

1. Create npm org **kortosystems** at [npmjs.com/org/create](https://www.npmjs.com/org/create)  
   *Or use `@ramairwing/proxy-engine` if you prefer your personal scope — update `distributions/npm-cli/package.json`.*
2. **Access Tokens** → **Automation** token with publish access
3. GitHub secret named exactly **`NPM_TOKEN`**

## Go-live sequence (first public release)

**Do not re-dispatch the tag until `NPM_TOKEN` is set.** `VSCE_PAT` is optional if you will upload the `.vsix` manually.

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
