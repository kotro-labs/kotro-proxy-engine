#!/usr/bin/env bash
# Re-dispatch a clean v* tag release after NPM_TOKEN and VSCE_PAT are configured.
#
# Usage:
#   scripts/go-live.sh v0.1.0
#   scripts/go-live.sh v0.1.0 --yes   # skip confirmation prompt
#
# Prerequisites (GitHub → Settings → Secrets → Actions):
#   - NPM_TOKEN
#   - VSCE_PAT
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TAG="${1:-}"
CONFIRM=0
[[ "${2:-}" == "--yes" ]] && CONFIRM=1

if [[ -z "$TAG" ]]; then
  echo "Usage: $0 <tag> [--yes]" >&2
  echo "Example: $0 v0.1.0" >&2
  exit 1
fi
[[ "$TAG" == v* ]] || TAG="v${TAG}"

if [[ "$CONFIRM" -eq 0 ]]; then
  echo "=== Go-live preflight ==="
  echo ""
  echo "Before re-dispatching ${TAG}, confirm these repository secrets exist:"
  echo "  https://github.com/ramairwing/kotro-proxy-engine/settings/secrets/actions"
  echo ""
  echo "  [ ] NPM_TOKEN  — npm automation token for @kortosystems/proxy-engine"
  echo "  [ ] VSCE_PAT   — VS Code Marketplace publish token (publisher: kortosystems)"
  echo ""
  echo "If either secret is missing, the release will build GitHub assets only"
  echo "and SKIP npm / Marketplace publish."
  echo ""
  read -r -p "Secrets configured? Type yes to re-dispatch ${TAG}: " answer
  if [[ "$answer" != "yes" ]]; then
    echo "Aborted. Add secrets first, then re-run: $0 ${TAG}"
    exit 1
  fi
fi

echo ""
echo "=== Step 1: Clear stale tag ==="
git tag -d "$TAG" 2>/dev/null || true
git push origin ":refs/tags/${TAG}" 2>/dev/null || true

echo ""
echo "=== Step 2: Re-stamp and dispatch release pipeline ==="
git tag "$TAG"
git push origin "$TAG"

echo ""
echo "=== Release dispatched ==="
echo "Monitor: https://github.com/ramairwing/kotro-proxy-engine/actions/workflows/release.yml"
echo ""
echo "When the workflow completes (~15–20 min), run:"
echo "  make post-release-homebrew VERSION=${TAG}"
echo ""
echo "Then publish the tap repo:"
echo "  cp distributions/homebrew-tap/Formula/kortolabs-proxy.rb ../homebrew-tap/Formula/"
echo "  cd ../homebrew-tap && git commit -am 'Bump kortolabs-proxy to ${TAG}' && git push"
