#!/usr/bin/env bash
set -e

# Kotro Proxy Installation Script
# Detects OS/arch, downloads the latest GitHub release binary, and installs it.
#
# Install location (in priority order):
#   1. ~/.local/bin   — no sudo required, works with `curl | bash`
#   2. /usr/local/bin — falls back to sudo if ~/.local/bin is not in PATH

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
    x86_64)     ARCH_TARGET="x86_64";;
    arm64|aarch64) ARCH_TARGET="aarch64";;
    *)          echo "Unsupported architecture: ${ARCH}" && exit 1;;
esac

TARGET="${ARCH_TARGET}-${OS_TARGET}"
ARTIFACT_NAME="kotro-proxy-${TARGET}"

# Choose install directory — prefer ~/.local/bin (no sudo needed)
if [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
    BIN_DIR="$HOME/.local/bin"
    USE_SUDO=0
else
    BIN_DIR="/usr/local/bin"
    USE_SUDO=1
fi

# Fetch latest release data
echo "Fetching latest release information..."
LATEST_RELEASE_URL="https://api.github.com/repos/${GITHUB_REPO}/releases/latest"
DOWNLOAD_URL=$(curl -sL "${LATEST_RELEASE_URL}" | grep "browser_download_url.*${ARTIFACT_NAME}.tar.gz" | cut -d : -f 2,3 | tr -d \" | xargs)

if [ -z "$DOWNLOAD_URL" ]; then
    echo "Could not find a release for ${TARGET}."
    echo "Please compile from source: cargo install --path rust/kotro-proxy"
    exit 1
fi

# Download and extract
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT
cd "${TMP_DIR}"

echo "Downloading ${ARTIFACT_NAME}..."
curl -sL "$DOWNLOAD_URL" -o "${ARTIFACT_NAME}.tar.gz"

echo "Extracting..."
tar -xzf "${ARTIFACT_NAME}.tar.gz"

echo "Installing to ${BIN_DIR}..."
if [ "$USE_SUDO" -eq 1 ]; then
    sudo mv "${ARTIFACT_NAME}" "${BIN_DIR}/kotro-proxy"
    sudo chmod +x "${BIN_DIR}/kotro-proxy"
else
    mv "${ARTIFACT_NAME}" "${BIN_DIR}/kotro-proxy"
    chmod +x "${BIN_DIR}/kotro-proxy"
fi

echo ""
echo "========================================="
echo "✅ Kotro Proxy Engine installed successfully!"
echo ""
echo "Binary: ${BIN_DIR}/kotro-proxy"
echo ""

# PATH hint if ~/.local/bin isn't already in PATH
if [ "$USE_SUDO" -eq 0 ] && ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
    echo "Add to your shell profile to use 'kotro-proxy' from anywhere:"
    echo '  echo '"'"'export PATH="$HOME/.local/bin:$PATH"'"'"' >> ~/.zshrc'
    echo "  source ~/.zshrc"
    echo ""
fi

echo "To start the proxy:"
echo "  kotro-proxy"
echo ""
echo "Then view your savings dashboard at http://localhost:9090/dashboard"
echo "========================================="
