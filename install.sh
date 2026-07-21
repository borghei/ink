#!/bin/sh
# ink installer — https://github.com/borghei/ink
# Usage: curl -fsSL https://raw.githubusercontent.com/borghei/ink/main/install.sh | sh

set -e

REPO="borghei/ink"
INSTALL_DIR="/usr/local/bin"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)   os="linux" ;;
  Darwin)  os="macos" ;;
  *)       echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)   arch="amd64" ;;
  arm64|aarch64)   arch="arm64" ;;
  *)               echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

BINARY="ink-${os}-${arch}"

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | cut -d'"' -f4)

if [ -z "$LATEST" ]; then
  echo "Could not determine latest release. Check https://github.com/$REPO/releases"
  exit 1
fi

URL="https://github.com/$REPO/releases/download/$LATEST/$BINARY"
SUMS_URL="https://github.com/$REPO/releases/download/$LATEST/SHA256SUMS"

echo "Downloading ink $LATEST for $os/$arch..."
TMPFILE=$(mktemp)
curl -fsSL "$URL" -o "$TMPFILE"

# Verify the download against the published checksum manifest before trusting it.
echo "Verifying checksum..."
EXPECTED=$(curl -fsSL "$SUMS_URL" | grep " $BINARY\$" | cut -d' ' -f1)
if [ -z "$EXPECTED" ]; then
  echo "Could not fetch checksum for $BINARY from $SUMS_URL — aborting."
  rm -f "$TMPFILE"
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL=$(sha256sum "$TMPFILE" | cut -d' ' -f1)
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL=$(shasum -a 256 "$TMPFILE" | cut -d' ' -f1)
else
  echo "Neither sha256sum nor shasum found — cannot verify download. Aborting."
  rm -f "$TMPFILE"
  exit 1
fi

if [ "$EXPECTED" != "$ACTUAL" ]; then
  echo "Checksum mismatch for $BINARY!"
  echo "  expected: $EXPECTED"
  echo "  actual:   $ACTUAL"
  rm -f "$TMPFILE"
  exit 1
fi
echo "Checksum OK."

chmod +x "$TMPFILE"

# Install
if [ -w "$INSTALL_DIR" ]; then
  mv "$TMPFILE" "$INSTALL_DIR/ink"
else
  echo "Installing to $INSTALL_DIR (requires sudo)..."
  sudo mv "$TMPFILE" "$INSTALL_DIR/ink"
fi

echo "ink $LATEST installed to $INSTALL_DIR/ink"
echo "Run 'ink --help' to get started."
