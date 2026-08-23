#!/usr/bin/env bash
set -e

# Kotro Proxy Installation Script
# Downloads the latest release binary from GitHub and installs it.
#
# Install location (in priority order):
#   1. ~/.local/bin   — no sudo required, works with `curl | bash`
#   2. /usr/local/bin — falls back here with sudo if ~/.local/bin unavailable

GITHUB_REPO="kotro-labs/kotro-proxy-engine"

echo "Installing Kotro Proxy Engine..."

# Detect OS
OS="$(uname -s)"
case "${OS}" in
    Linux*)     OS_TARGET="unknown-linux-gnu";;
    Darwin*)    OS_TARGET="apple-darwin";;
    *)          echo "Unsupported OS: ${OS}" && exit 1;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
    x86_64)        ARCH_TARGET="x86_64";;
    arm64|aarch64) ARCH_TARGET="aarch64";;
    *)             echo "Unsupported architecture: ${ARCH}" && exit 1;;
esac

TARGET="${ARCH_TARGET}-${OS_TARGET}"
ARTIFACT_NAME="kotro-proxy-${TARGET}"

# Use GitHub's releases/latest/download/ redirect — no JSON parsing needed
DOWNLOAD_URL="https://github.com/${GITHUB_REPO}/releases/latest/download/${ARTIFACT_NAME}.tar.gz"

# Choose install directory — prefer ~/.local/bin (no sudo needed)
if mkdir -p "$HOME/.local/bin" 2>/dev/null; then
    BIN_DIR="$HOME/.local/bin"
    USE_SUDO=0
else
    BIN_DIR="/usr/local/bin"
    USE_SUDO=1
fi

# Download and extract
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT
cd "${TMP_DIR}"

echo "Downloading ${ARTIFACT_NAME} from ${DOWNLOAD_URL}..."
if ! curl -fSL --progress-bar "$DOWNLOAD_URL" -o "${ARTIFACT_NAME}.tar.gz"; then
    echo ""
    echo "Download failed. Check your network or visit:"
    echo "  https://github.com/${GITHUB_REPO}/releases/latest"
    exit 1
fi

echo "Extracting..."
tar -xzf "${ARTIFACT_NAME}.tar.gz"

# Find the binary (may be named kotro-proxy-<target> or kotro-proxy)
BINARY=$(find . -maxdepth 1 -type f -name "kotro-proxy*" ! -name "*.tar.gz" | head -1)
if [ -z "$BINARY" ]; then
    echo "Could not find kotro-proxy binary in tarball. Contents:"
    ls -la
    exit 1
fi

echo "Installing to ${BIN_DIR}..."
if [ "$USE_SUDO" -eq 1 ]; then
    sudo mv "$BINARY" "${BIN_DIR}/kotro-proxy"
    sudo chmod +x "${BIN_DIR}/kotro-proxy"
else
    mv "$BINARY" "${BIN_DIR}/kotro-proxy"
    chmod +x "${BIN_DIR}/kotro-proxy"
fi

echo ""
echo "========================================="
echo "✅ Kotro Proxy Engine installed!"
echo ""
echo "Binary: ${BIN_DIR}/kotro-proxy"
echo ""

# PATH hint if ~/.local/bin isn't already on PATH
if [ "$USE_SUDO" -eq 0 ] && ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
    echo "Add to your shell to use 'kotro-proxy' from anywhere:"
    SHELL_RC="$HOME/.zshrc"
    [ -n "$BASH_VERSION" ] && SHELL_RC="$HOME/.bashrc"
    echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ${SHELL_RC}"
    echo "  source ${SHELL_RC}"
    echo ""
fi

echo "Next steps:"
echo "  # Real provider traffic (set both):"
echo "  KOTRO_UPSTREAM_URL=https://api.openai.com KOTRO_UPSTREAM_API_KEY=\$OPENAI_API_KEY kotro-proxy"
echo ""
echo "  # No API key? Bare kotro-proxy will not serve a demo on :9000."
echo "  # Clone the repo and run the no-key proof:"
echo "  #   git clone https://github.com/kotro-labs/kotro-proxy-engine.git && cd kotro-proxy-engine"
echo "  #   make demo-injection"
echo "  # Docs: https://github.com/kotro-labs/kotro-proxy-engine#see-it-yourself-no-api-key"
echo ""
echo "Dashboard (after proxy is up with an upstream): http://localhost:9090/dashboard"
echo "========================================="
