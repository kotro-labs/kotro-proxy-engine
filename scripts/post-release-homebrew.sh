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

if ! git config user.email >/dev/null 2>&1 || ! git config user.name >/dev/null 2>&1; then
  echo "Git author identity is not configured." >&2
  echo "" >&2
  echo "Set it once (repo-local example):" >&2
  echo '  git config user.email "you@example.com"' >&2
  echo '  git config user.name "Your Name"' >&2
  echo "" >&2
  echo "Formula files are already staged. Then run:" >&2
  echo "  git commit -m \"chore: align production homebrew tap checksums for ${TAG#v}\"" >&2
  echo "  git push origin main" >&2
  exit 1
fi

git commit -m "chore: align production homebrew tap checksums for ${TAG#v}"

echo ""
echo "Committed formula checksums. Push when ready:"
echo "  git push origin main"
