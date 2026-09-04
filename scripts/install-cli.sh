#!/usr/bin/env bash
set -euo pipefail

# Install the bundled sidecar CLI into a user-writable PATH directory.
APP_PATH="${1:-/Applications/矩龙破甲.app}"
CLI="${APP_PATH}/Contents/MacOS/julong-codex"
if [[ ! -x "$CLI" ]]; then
  echo "CLI not found: $CLI"
  echo "Install the macOS App first or pass its .app path as the first argument."
  exit 1
fi

BIN_DIR="${HOME}/.local/bin"
mkdir -p "$BIN_DIR"
ln -sfn "$CLI" "$BIN_DIR/julong-codex"
echo "[OK] ${BIN_DIR}/julong-codex -> ${CLI}"
if [[ ":${PATH}:" != *":${BIN_DIR}:"* ]]; then
  echo "Add this line to ~/.zshrc (or ~/.bashrc):"
  echo "export PATH=\"\$HOME/.local/bin:\$PATH\""
  echo "Then run: source ~/.zshrc"
fi
echo "Test with: julong-codex status"
