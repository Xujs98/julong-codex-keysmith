#!/usr/bin/env bash
set -euo pipefail

# macOS 本机交叉构建 Windows 交付物。默认生成 x64 NSIS 安装程序。

MODE="${1:-nsis}"
ARCH="${2:-x64}"

if [[ "$MODE" == "x64" || "$MODE" == "arm64" ]]; then
  ARCH="$MODE"
  MODE="nsis"
fi

if [[ "$MODE" == "-h" || "$MODE" == "--help" ]]; then
  echo "Usage: $0 [nsis|exe] [x64|arm64]"
  echo "Default: $0 nsis x64"
  exit 0
fi

case "$MODE" in
  nsis|exe) ;;
  *) echo "Usage: $0 [nsis|exe] [x64|arm64]"; exit 2 ;;
esac

case "$ARCH" in
  x64) TARGET="x86_64-pc-windows-msvc" ;;
  arm64) TARGET="aarch64-pc-windows-msvc" ;;
  *) echo "Usage: $0 [nsis|exe] [x64|arm64]"; exit 2 ;;
esac

[[ "$(uname -s)" == "Darwin" ]] || { echo "Run this script on macOS."; exit 1; }

# Finder 和非交互 Shell 通常没有加载 nvm。
if ! command -v node >/dev/null 2>&1; then
  NVM_NODE_BIN="$(find "${HOME}/.nvm/versions/node" -maxdepth 3 -type f -path '*/bin/node' -print 2>/dev/null | sort | tail -n 1 | xargs -n 1 dirname 2>/dev/null || true)"
  [[ -n "$NVM_NODE_BIN" ]] && export PATH="$NVM_NODE_BIN:$PATH"
fi

# Homebrew LLVM 是 keg-only，Windows 资源编译需要其中的 llvm-rc。
if ! command -v llvm-rc >/dev/null 2>&1 && command -v brew >/dev/null 2>&1; then
  LLVM_BIN="$(brew --prefix llvm 2>/dev/null || true)/bin"
  [[ -x "$LLVM_BIN/llvm-rc" ]] && export PATH="$LLVM_BIN:$PATH"
fi

require_command() {
  command -v "$1" >/dev/null 2>&1 && return
  echo "Missing required command: $1"
  case "$1" in
    cargo-xwin) echo "Install with: cargo install cargo-xwin" ;;
    llvm-rc) echo "Install with: brew install llvm" ;;
    makensis) echo "Install with: brew install nsis" ;;
  esac
  exit 1
}

for command_name in node npm npx cargo cargo-xwin rustup llvm-rc; do
  require_command "$command_name"
done
[[ "$MODE" == "nsis" ]] && require_command makensis

echo "=== Windows $MODE build ($TARGET) ==="
if [[ -f package-lock.json ]]; then npm ci; else npm install; fi
rustup target add "$TARGET"

# prepare-sidecar.mjs 调用普通 cargo，因此先导入 cargo-xwin 的 SDK、链接器和头文件环境。
PATH_BEFORE_XWIN="$PATH"
eval "$(cargo-xwin env --target "$TARGET")"
export PATH="$PATH_BEFORE_XWIN:$PATH"
node scripts/prepare-sidecar.mjs "$TARGET"

RELEASE_DIR="src-tauri/target/$TARGET/release"
if [[ "$MODE" == "nsis" ]]; then
  npx tauri build \
    --runner cargo-xwin \
    --config src-tauri/tauri.sidecar.conf.json \
    --target "$TARGET" \
    --bundles nsis \
    --ci

  BUNDLE_DIR="$RELEASE_DIR/bundle/nsis"
  INSTALLER="$(find "$BUNDLE_DIR" -maxdepth 1 -type f -name '*.exe' -print -quit 2>/dev/null || true)"
  [[ -n "$INSTALLER" ]] || { echo "NSIS installer not found under $BUNDLE_DIR"; exit 1; }
  find "$BUNDLE_DIR" -maxdepth 1 -type f -name '*.exe' -print | sed 's#^#[OK] #'
  exit 0
fi

cargo-xwin build --manifest-path src-tauri/Cargo.toml --target "$TARGET" --release

SOURCE_EXE="$RELEASE_DIR/julong-codex-keysmith.exe"
[[ -f "$SOURCE_EXE" ]] || SOURCE_EXE="$RELEASE_DIR/super-instruct.exe"
[[ -f "$SOURCE_EXE" ]] || SOURCE_EXE="$RELEASE_DIR/矩龙破甲.exe"
CLI_EXE="$RELEASE_DIR/julong-codex.exe"
[[ -f "$SOURCE_EXE" ]] || { echo "Windows executable not found under $RELEASE_DIR"; exit 1; }
[[ -f "$CLI_EXE" ]] || { echo "Windows CLI executable not found: $CLI_EXE"; exit 1; }

OUT_DIR="artifacts/windows-local"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
cp "$SOURCE_EXE" "$OUT_DIR/矩龙破甲.exe"
cp "$CLI_EXE" "$OUT_DIR/julong-codex.exe"
cp bridge.md "$OUT_DIR/bridge.md"
cp -R codex-skills "$OUT_DIR/codex-skills"
cp -R mcp-tools "$OUT_DIR/mcp-tools"

echo "[OK] $OUT_DIR/矩龙破甲.exe"
echo "[OK] $OUT_DIR/julong-codex.exe"
echo "[OK] $OUT_DIR/bridge.md"
echo "[OK] $OUT_DIR/codex-skills/"
echo "[OK] $OUT_DIR/mcp-tools/"
