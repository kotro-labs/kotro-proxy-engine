#!/usr/bin/env bash
# Fetch GitHub release tarballs and stamp SHA-256 checksums into Homebrew formulas.
#
# Usage:
#   scripts/update-homebrew-shas.sh v0.1.0
#   scripts/update-homebrew-shas.sh v0.1.0 --dry-run
#
# Updates:
#   distributions/homebrew/Formula/kortolabs-proxy.rb
#   distributions/homebrew-tap/Formula/kortolabs-proxy.rb
#
# Requires: gh (authenticated against the release repository)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="${GITHUB_REPO:-ramairwing/kotro-proxy-engine}"
FORMULAS=(
  "${ROOT}/distributions/homebrew/Formula/kortolabs-proxy.rb"
  "${ROOT}/distributions/homebrew-tap/Formula/kortolabs-proxy.rb"
)

DRY_RUN=0
TAG=""

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    -h|--help)
      echo "Usage: $0 <tag> [--dry-run]"
      exit 0
      ;;
    -*)
      echo "Unknown flag: $arg" >&2
      exit 1
      ;;
    *)
      [[ -z "$TAG" ]] || { echo "Unexpected argument: $arg" >&2; exit 1; }
      TAG="$arg"
      ;;
  esac
done

[[ -n "$TAG" ]] || { echo "Usage: $0 <tag> [--dry-run]" >&2; exit 1; }
[[ "$TAG" == v* ]] || TAG="v${TAG}"
VERSION="${TAG#v}"

command -v gh >/dev/null 2>&1 || { echo "gh CLI not found. Install: brew install gh" >&2; exit 1; }

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

ARM_ASSET="korto-proxy-aarch64-apple-darwin.tar.gz"
INTEL_ASSET="korto-proxy-x86_64-apple-darwin.tar.gz"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "=== Downloading ${TAG} assets from ${REPO} ==="
if ! gh release view "$TAG" -R "$REPO" >/dev/null 2>&1; then
  echo "Release ${TAG} not found. Wait for the release workflow to finish." >&2
  exit 1
fi

gh release download "$TAG" -R "$REPO" -D "$TMP" -p "$ARM_ASSET" -p "$INTEL_ASSET"

ARM_SHA="$(sha256_file "${TMP}/${ARM_ASSET}")"
INTEL_SHA="$(sha256_file "${TMP}/${INTEL_ASSET}")"

echo ""
echo "${ARM_ASSET}:  ${ARM_SHA}"
echo "${INTEL_ASSET}: ${INTEL_SHA}"
echo ""

export VERSION TAG ARM_SHA INTEL_SHA ARM_ASSET INTEL_ASSET

update_formula() {
  local formula="$1"
  [[ -f "$formula" ]] || { echo "Skip missing: $formula" >&2; return; }

  cp "$formula" "${formula}.bak"

  perl -i -pe '
    s/^  version ".*"/  version "$ENV{VERSION}"/;
    s|releases/download/v[^/]+/|releases/download/$ENV{TAG}/|g;
    s/PLACEHOLDER_SHA256_AARCH64_APPLE_DARWIN/$ENV{ARM_SHA}/;
    s/PLACEHOLDER_SHA256_X86_64_APPLE_DARWIN/$ENV{INTEL_SHA}/;
  ' "$formula"

  perl -i -0pe '
    s/(url ".*$ENV{ARM_ASSET}")\n      sha256 "[a-f0-9]{64}"/$1\n      sha256 "$ENV{ARM_SHA}"/s;
    s/(url ".*$ENV{INTEL_ASSET}")\n      sha256 "[a-f0-9]{64}"/$1\n      sha256 "$ENV{INTEL_SHA}"/s;
  ' "$formula"

  echo "=== ${formula} ==="
  diff -u "${formula}.bak" "$formula" || true
  rm -f "${formula}.bak"
}

for formula in "${FORMULAS[@]}"; do
  update_formula "$formula"
done

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo ""
  echo "Dry run complete (changes reverted)."
  git -C "$ROOT" checkout -- distributions/homebrew/Formula/kortolabs-proxy.rb distributions/homebrew-tap/Formula/kortolabs-proxy.rb
  exit 0
fi

echo ""
echo "Updated Homebrew formulas. Sync homebrew-tap repo:"
echo "  cp distributions/homebrew-tap/Formula/kortolabs-proxy.rb ../homebrew-tap/Formula/"
