#!/usr/bin/env bash
# Stamp Homebrew SHA-256 checksums after a GitHub release completes.
#
# Usage:
#   scripts/post-release-homebrew.sh v0.1.0
#   make post-release-homebrew VERSION=v0.1.0
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAG="${1:-}"

if [[ -z "$TAG" ]]; then
  echo "Usage: $0 <tag>" >&2
  echo "Example: $0 v0.1.0" >&2
  exit 1
fi

bash "${ROOT}/scripts/update-homebrew-shas.sh" "$TAG"

cd "$ROOT"
git add distributions/homebrew/Formula/kortolabs-proxy.rb distributions/homebrew-tap/Formula/kortolabs-proxy.rb

if git diff --cached --quiet; then
  echo "No formula changes to commit."
  exit 0
fi

git commit -m "chore: align production homebrew tap checksums for ${TAG#v}"

echo ""
echo "Committed formula checksums. Push when ready:"
echo "  git push origin main"
