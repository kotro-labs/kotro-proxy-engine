# Distribution registry secrets

Configure these in **GitHub → Settings → Secrets and variables → Actions** on `ramairwing/kotro-proxy-engine`.

| Secret | Purpose | How to obtain |
|--------|---------|---------------|
| `NPM_TOKEN` | Publish `@kortolabs/proxy-engine` on tag push | [npmjs.com](https://www.npmjs.com) → Access Tokens → **Automation** token |
| `VSCE_PAT` | Publish `kortolabs.kortolabs-proxy-engine` to VS Code Marketplace | [Azure DevOps PAT](https://dev.azure.com) with **Marketplace → Manage** scope, or `vsce login` PAT |

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
