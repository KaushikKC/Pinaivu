#!/bin/sh
set -e

REPO="KaushikKC/Pinaivu"
INSTALL_DIR="/usr/local/bin"
BINARY="pinaivu"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64) ARTIFACT="pinaivu-linux-x86_64" ;;
      *)       echo "Unsupported Linux architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      arm64)  ARTIFACT="pinaivu-macos-apple-silicon" ;;
      x86_64) ARTIFACT="pinaivu-macos-intel" ;;
      *)      echo "Unsupported Mac architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS"
    echo "Windows: download from https://github.com/$REPO/releases/latest"
    exit 1
    ;;
esac

# Get latest release version
echo "Fetching latest Pinaivu release..."
VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$VERSION" ]; then
  echo "Could not determine latest version. Check https://github.com/$REPO/releases"
  exit 1
fi

echo "Installing Pinaivu $VERSION for $OS/$ARCH..."

URL="https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT"
TMP="$(mktemp)"

curl -fsSL "$URL" -o "$TMP"
chmod +x "$TMP"
sudo mv "$TMP" "$INSTALL_DIR/$BINARY"

echo ""
echo "Pinaivu $VERSION installed to $INSTALL_DIR/$BINARY"
echo ""
echo "Next steps:"
echo "  1. Install Ollama:  https://ollama.com"
echo "  2. Pull a model:    ollama pull deepseek-r1:7b"
echo "  3. Init your node:  pinaivu init"
echo "  4. Start the node:  pinaivu start"
echo ""
echo "Your node will automatically join the Pinaivu network."
