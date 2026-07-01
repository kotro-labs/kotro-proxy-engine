# Distribution registry secrets

Configure these in **GitHub → Settings → Secrets and variables → Actions** on `ramairwing/kotro-proxy-engine`.

| Secret | Purpose | How to obtain |
|--------|---------|---------------|
| `NPM_TOKEN` | Publish `@kortolabs/proxy-engine` on tag push | [npmjs.com](https://www.npmjs.com) → Access Tokens → **Automation** token |
| `VSCE_PAT` | Publish `kortolabs.kortolabs-proxy-engine` to VS Code Marketplace | [Azure DevOps PAT](https://dev.azure.com) with **Marketplace → Manage** scope, or `vsce login` PAT |

## Go-live sequence (first public release)

**Do not re-dispatch the tag until both secrets are active.** If the pipeline runs without them, GitHub Release assets are still built, but npm and Marketplace publish steps skip.

### 0. Add secrets

1. [Repository secrets dashboard](https://github.com/ramairwing/kotro-proxy-engine/settings/secrets/actions)
2. Add `NPM_TOKEN` and `VSCE_PAT` (see setup sections below)

### 1. Re-dispatch tag (after secrets are live)

```bash
make go-live VERSION=v0.1.0
# or
scripts/go-live.sh v0.1.0
```

This deletes the stale `v0.1.0` tag locally and on `origin`, then pushes a fresh tag to trigger the full release matrix including registry publish.

### 2. Monitor CI

https://github.com/ramairwing/kotro-proxy-engine/actions/workflows/release.yml

Verify jobs complete and assets appear at:
https://github.com/ramairwing/kotro-proxy-engine/releases/tag/v0.1.0

### 3. Stamp Homebrew checksums (after release assets upload)

```bash
make post-release-homebrew VERSION=v0.1.0
git push origin main
```

### 4. Publish Homebrew tap

```bash
cp distributions/homebrew-tap/Formula/kortolabs-proxy.rb ../homebrew-tap/Formula/
cd ../homebrew-tap && git commit -am "Bump kortolabs-proxy to v0.1.0" && git push
```

### 5. Verify public telemetry

| Surface | URL |
|---------|-----|
| GitHub Release | https://github.com/ramairwing/kotro-proxy-engine/releases |
| npm | https://www.npmjs.com/package/@kortolabs/proxy-engine |
| VS Code Marketplace | https://marketplace.visualstudio.com/items?itemName=kortolabs.kortolabs-proxy-engine |

---

## npm setup

1. Create npm org/user `@kortolabs` (or update `distributions/npm-cli/package.json` name).
2. Add `NPM_TOKEN` secret to the repository.
3. Push a `v*` tag — workflow publishes from `distributions/npm-cli/`.

## VS Code Marketplace setup

1. Create publisher at [marketplace.visualstudio.com/manage](https://marketplace.visualstudio.com/manage) (publisher id: `kortolabs`).
2. Generate PAT with **Marketplace (Publish)** scope.
3. Add as `VSCE_PAT` repository secret.
4. Push a `v*` tag — workflow runs `vsce publish` with native binaries embedded.

If secrets are absent, the release workflow **skips** registry publish and still uploads GitHub Release assets + `.vsix`.

## Homebrew tap

No GitHub secret required. After release:

```bash
scripts/update-homebrew-shas.sh v0.1.0
```

Sync `distributions/homebrew-tap/Formula/` into `github.com/ramairwing/homebrew-tap`.
