#!/usr/bin/env bash
# Cross-compile Go proxy binaries for npm / VS Code extension distribution names.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${1:-${ROOT}/dist-go}"

mkdir -p "$OUT_DIR"

build_one() {
  local goos="$1"
  local goarch="$2"
  local out="$3"
  echo "→ GOOS=$goos GOARCH=$goarch $out"
  GOOS="$goos" GOARCH="$goarch" CGO_ENABLED=0 \
    go build -trimpath -ldflags="-s -w" \
    -o "${OUT_DIR}/${out}" "${ROOT}/cmd/proxy"
  chmod +x "${OUT_DIR}/${out}" 2>/dev/null || true
}

build_one darwin arm64 korto-proxy-aarch64-apple-darwin
build_one darwin amd64 korto-proxy-x86_64-apple-darwin
build_one linux amd64 korto-proxy-x86_64-unknown-linux-gnu
build_one windows amd64 korto-proxy-x86_64-pc-windows-msvc.exe

ls -la "$OUT_DIR"
