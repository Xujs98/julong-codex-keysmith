#!/usr/bin/env bash
set -euo pipefail

# Install the sidecar CLI into a directory already present in PATH when
# possible. The repository's debug/release binaries are supported so this
# also fixes `command not found` during local development.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -gt 0 ]]; then
  CANDIDATES=("$1/Contents/MacOS/julong-codex")
else
  CANDIDATES=(
    "/Applications/矩龙破甲.app/Contents/MacOS/julong-codex"
    "$ROOT/src-tauri/target/x86_64-apple-darwin/release/julong-codex"
    "$ROOT/src-tauri/target/release/julong-codex"
    "$ROOT/src-tauri/target/debug/julong-codex"
  )
fi

CLI=""
for candidate in "${CANDIDATES[@]}"; do
  if [[ -x "$candidate" ]]; then
    CLI="$candidate"
    break
  fi
done

if [[ -z "$CLI" ]]; then
  echo "julong-codex CLI not found."
  if [[ $# -gt 0 ]]; then
    echo "Checked App path: $1"
  else
    echo "Install the macOS App, or build it with:"
    echo "cargo build --manifest-path src-tauri/Cargo.toml --bin julong-codex"
  fi
  exit 1
fi

if [[ -n "${JULONG_CODEX_BIN_DIR:-}" ]]; then
  BIN_DIR="$JULONG_CODEX_BIN_DIR"
else
  BIN_DIR=""
  IFS=: read -r -a PATH_DIRS <<< "${PATH:-}"
  for candidate in "${PATH_DIRS[@]}"; do
    if [[ -n "$candidate" && -d "$candidate" && -w "$candidate" ]]; then
      BIN_DIR="$candidate"
      break
    fi
  done
  BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
fi

mkdir -p "$BIN_DIR"
TARGET="$BIN_DIR/julong-codex"
if [[ -e "$TARGET" && ! -L "$TARGET" ]]; then
  echo "Refusing to replace an existing file: $TARGET"
  exit 1
fi
ln -sfn "$CLI" "$TARGET"
echo "[OK] $TARGET -> $CLI"
if [[ ":${PATH:-}:" != *":${BIN_DIR}:"* ]]; then
  echo "Add this line to ~/.zshrc (or ~/.bashrc):"
  echo "export PATH=\"$BIN_DIR:\$PATH\""
  echo "Then run: source ~/.zshrc"
fi
echo "Test with: julong-codex status"
