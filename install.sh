#!/usr/bin/env bash
set -euo pipefail

# Clipman installer script
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/dsnbyte/clipman/main/install.sh | bash

REPO="dsnbyte/clipman"
BINARY_NAME="clipman"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"
INSTALL_DIR="${CLIPMAN_INSTALL_DIR:-${BINDIR:-$DEFAULT_INSTALL_DIR}}"

BOLD="\033[1m"
GREEN="\033[0;32m"
BLUE="\033[0;34m"
YELLOW="\033[1;33m"
RED="\033[0;31m"
RESET="\033[0m"

info() {
    printf "${BLUE}==>${RESET} ${BOLD}%s${RESET}\n" "$1"
}

success() {
    printf "${GREEN}==>${RESET} ${BOLD}%s${RESET}\n" "$1"
}

warn() {
    printf "${YELLOW}warning:${RESET} %s\n" "$1"
}

error() {
    printf "${RED}error:${RESET} %s\n" "$1" >&2
    exit 1
}

# 1. Check OS and Architecture
OS="$(uname -s)"
if [ "$OS" != "Linux" ]; then
    error "Clipman is currently only built for Linux systems (detected: $OS)."
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        TARGET="x86_64-unknown-linux-gnu"
        ;;
    *)
        error "Prebuilt binaries are currently only provided for x86_64 (detected: $ARCH). Please build from source using 'cargo build --release'."
        ;;
esac

# 2. Check prerequisites
for cmd in curl tar; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        error "Required command '$cmd' is not installed."
    fi
done

# 3. Determine download URL
VERSION="${CLIPMAN_VERSION:-latest}"
if [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/clipman-${TARGET}.tar.gz"
else
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/clipman-${TARGET}.tar.gz"
fi

info "Installing Clipman (${TARGET})..."

# 4. Download and extract
TMP_DIR="$(mktemp -d -t clipman-install.XXXXXX)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

ARCHIVE_PATH="${TMP_DIR}/clipman.tar.gz"
info "Downloading prebuilt release from GitHub..."
HTTP_STATUS=$(curl -fsSL -w "%{http_code}" -o "$ARCHIVE_PATH" "$DOWNLOAD_URL" || true)

if [ ! -s "$ARCHIVE_PATH" ]; then
    error "Failed to download Clipman release from:
  $DOWNLOAD_URL
If no release has been published yet, you can build from source:
  git clone https://github.com/${REPO}.git && cd clipman && cargo build --release"
fi

tar -xzf "$ARCHIVE_PATH" -C "$TMP_DIR"

if [ ! -f "${TMP_DIR}/${BINARY_NAME}" ]; then
    error "Release archive did not contain the '${BINARY_NAME}' executable."
fi

# 5. Place binary in target directory
mkdir -p "$INSTALL_DIR"
cp "${TMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

success "Binary installed to ${INSTALL_DIR}/${BINARY_NAME}"

# 6. Run desktop and shortcut integration
info "Registering desktop launcher, autostart, and default keybinding..."
if "${INSTALL_DIR}/${BINARY_NAME}" install; then
    success "Desktop integration completed successfully."
else
    warn "Desktop integration finished with warnings (non-GNOME environments may require manual shortcut configuration)."
fi

# 7. Check PATH
case ":$PATH:" in
    *":${INSTALL_DIR}:"*)
        ;;
    *)
        echo ""
        warn "${INSTALL_DIR} is not in your \$PATH."
        echo "  Add it to your shell configuration file (~/.bashrc, ~/.zshrc, etc.):"
        echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
        ;;
esac

echo ""
printf "${GREEN}${BOLD}Clipman is ready to use!${RESET}\n"
echo "  • Open history: Press Ctrl + Alt + V (or run '${BINARY_NAME} toggle')"
echo "  • Start daemon: Run '${BINARY_NAME} daemon &' (automatically started on login)"
echo "  • Open settings: Run '${BINARY_NAME} settings'"
echo ""
