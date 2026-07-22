#!/usr/bin/env bash
set -euo pipefail

REPO="vitebc/mcp-1c-help"
BIN_NAME="mcp-1c-help"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}==>${NC} $1"; }
warn()  { echo -e "${YELLOW}==>${NC} $1"; }
err()   { echo -e "${RED}==>${NC} $1" >&2; }

# --- detect platform ---
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) err "Unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    linux)   OS="linux" ;;
    darwin)  OS="macos" ;;
    *) err "Unsupported OS: $OS"; exit 1 ;;
esac

ASSET="${BIN_NAME}-${OS}-${ARCH}.tar.gz"
TAG="${TAG:-latest}"

# --- resolve download url ---
if [ "$TAG" = "latest" ]; then
    API_URL="https://api.github.com/repos/$REPO/releases/latest"
    DOWNLOAD_URL=$(curl -sSL "$API_URL" | grep "browser_download_url" | grep "$ASSET" | head -1 | cut -d'"' -f4)
    if [ -z "$DOWNLOAD_URL" ]; then
        # try unarchived binary fallback
        API_URL="https://api.github.com/repos/$REPO/releases/latest"
        DOWNLOAD_URL=$(curl -sSL "$API_URL" | grep "browser_download_url" | grep "${BIN_NAME}-${OS}-${ARCH}" | head -1 | cut -d'"' -f4)
    fi
else
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/$ASSET"
fi

if [ -z "$DOWNLOAD_URL" ]; then
    err "Could not find release asset: $ASSET"
    err "Check releases at: https://github.com/$REPO/releases"
    exit 1
fi

# --- download ---
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

info "Downloading $BIN_NAME $TAG..."
curl -sSL "$DOWNLOAD_URL" -o "$TMPDIR/$ASSET"

# --- extract ---
info "Extracting..."
tar xzf "$TMPDIR/$ASSET" -C "$TMPDIR"

# --- install ---
if [ ! -d "$INSTALL_DIR" ]; then
    warn "Creating $INSTALL_DIR (may need sudo)"
    mkdir -p "$INSTALL_DIR" 2>/dev/null || sudo mkdir -p "$INSTALL_DIR"
fi

if [ -w "$INSTALL_DIR" ]; then
    mv "$TMPDIR/$BIN_NAME" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/$BIN_NAME"
else
    info "Installing to $INSTALL_DIR (requires sudo)..."
    sudo mv "$TMPDIR/$BIN_NAME" "$INSTALL_DIR/"
    sudo chmod +x "$INSTALL_DIR/$BIN_NAME"
fi

info "Installed to $INSTALL_DIR/$BIN_NAME"

# --- verify ---
if command -v "$BIN_NAME" &>/dev/null; then
    info "Run '$BIN_NAME --help' to get started"
else
    warn "Make sure $INSTALL_DIR is in your PATH: export PATH=\"\$PATH:$INSTALL_DIR\""
fi
