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
# Requires: curl. Uses gh CLI when available (recommended for private repos).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="${GITHUB_REPO:-kotro-labs/kotro-proxy-engine}"
FORMULAS=(
  "${ROOT}/distributions/homebrew/Formula/kortolabs-proxy.rb"
  "${ROOT}/distributions/homebrew-tap/Formula/kortolabs-proxy.rb"
)

DRY_RUN=0
FROM_DIR=""
TAG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --from-dir)
      shift
      [[ $# -gt 0 ]] || { echo "--from-dir requires a path" >&2; exit 1; }
      FROM_DIR="$1"
      shift
      ;;
    -h|--help)
      echo "Usage: $0 <tag> [--dry-run] [--from-dir <path>]"
      echo ""
      echo "  --from-dir  Use local tarballs (e.g. downloaded from GitHub Actions artifacts)"
      exit 0
      ;;
    -*)
      echo "Unknown flag: $1" >&2
      exit 1
      ;;
    *)
      [[ -z "$TAG" ]] || { echo "Unexpected argument: $1" >&2; exit 1; }
      TAG="$1"
      shift
      ;;
  esac
done

[[ -n "$TAG" ]] || { echo "Usage: $0 <tag> [--dry-run]" >&2; exit 1; }
[[ "$TAG" == v* ]] || TAG="v${TAG}"
VERSION="${TAG#v}"

command -v curl >/dev/null 2>&1 || { echo "curl not found" >&2; exit 1; }

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

ARM_ASSET="korto-proxy-aarch64-apple-darwin.tar.gz"
INTEL_ASSET="korto-proxy-x86_64-apple-darwin.tar.gz"

download_asset() {
  local asset="$1"
  local dest="${TMP}/${asset}"

  if [[ -n "$FROM_DIR" ]]; then
    cp "${FROM_DIR}/${asset}" "$dest"
    return
  fi

  if command -v gh >/dev/null 2>&1; then
    gh release download "$TAG" -R "$REPO" -D "$TMP" -p "$asset"
    return
  fi

  local url="https://github.com/${REPO}/releases/download/${TAG}/${asset}"
  echo "Fetching ${url}"
  if ! curl -fsSL "$url" -o "$dest"; then
    echo "" >&2
    echo "Download failed. For private repos, install gh and authenticate:" >&2
    echo "  brew install gh && gh auth login" >&2
    echo "  $0 ${TAG}" >&2
    echo "" >&2
    echo "Or download macOS .tar.gz assets from the CI run and pass:" >&2
    echo "  $0 ${TAG} --from-dir /path/to/downloads" >&2
    exit 1
  fi
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "=== Resolving ${TAG} Homebrew tarballs from ${REPO} ==="
download_asset "$ARM_ASSET"
download_asset "$INTEL_ASSET"

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
