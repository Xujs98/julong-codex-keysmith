#!/usr/bin/env bash
set -euo pipefail

# macOS 本机交叉编译 Windows 可执行文件。
# 产物：artifacts/windows-local/矩龙破甲.exe

ARCH="${1:-x64}"
case "$ARCH" in
  x64) TARGET="x86_64-pc-windows-msvc" ;;
  arm64) TARGET="aarch64-pc-windows-msvc" ;;
  *) echo "Usage: $0 [x64|arm64]"; exit 2 ;;
esac

[[ "$(uname -s)" == "Darwin" ]] || { echo "Run this script on macOS."; exit 1; }

# 从 Finder / 非交互 shell 调用时，nvm 的 Node 通常不在 PATH。自动发现一个本地
# nvm 版本，保持用户只需执行一条构建命令。
if ! command -v node >/dev/null 2>&1; then
  NVM_NODE_BIN="$(find "${HOME}/.nvm/versions/node" -maxdepth 3 -type f -path '*/bin/node' -print 2>/dev/null | sort | tail -n 1 | xargs -n 1 dirname 2>/dev/null || true)"
  [[ -n "${NVM_NODE_BIN}" ]] && export PATH="${NVM_NODE_BIN}:${PATH}"
fi

# Tauri 的 Windows 资源编译需要 llvm-rc。Homebrew LLVM 是 keg-only，因此同样
# 自动加入 PATH；未安装时给出一次性的准确安装命令。
if ! command -v llvm-rc >/dev/null 2>&1 && command -v brew >/dev/null 2>&1; then
  LLVM_BIN="$(brew --prefix llvm 2>/dev/null || true)/bin"
  [[ -x "${LLVM_BIN}/llvm-rc" ]] && export PATH="${LLVM_BIN}:${PATH}"
fi
command -v llvm-rc >/dev/null 2>&1 || {
  echo "Missing required command: llvm-rc"
  echo "Install with: brew install llvm"
  exit 1
}

for command_name in cargo cargo-xwin rustup; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "Missing required command: $command_name"
    [[ "$command_name" == "cargo-xwin" ]] && echo "Install with: cargo install cargo-xwin"
    exit 1
  }
done

if command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1; then
  if [[ -f package-lock.json ]]; then npm ci; else npm install; fi
  npm run copy-resources
else
  echo "Node/npm not found; using checked-in frontend and embedded bridge fallback."
fi

rustup target add "$TARGET"

echo "Building Windows executable locally: $TARGET"
cargo xwin build \
  --manifest-path src-tauri/Cargo.toml \
  --target "$TARGET" \
  --release

RELEASE_DIR="src-tauri/target/$TARGET/release"
SOURCE_EXE="$RELEASE_DIR/julong-codex-keysmith.exe"
if [[ ! -f "$SOURCE_EXE" ]]; then
  SOURCE_EXE="$RELEASE_DIR/super-instruct.exe"
fi
if [[ ! -f "$SOURCE_EXE" ]]; then
  SOURCE_EXE="$RELEASE_DIR/矩龙破甲.exe"
fi
[[ -f "$SOURCE_EXE" ]] || { echo "Windows executable not found under $RELEASE_DIR"; exit 1; }

OUT_DIR="artifacts/windows-local"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
cp "$SOURCE_EXE" "$OUT_DIR/矩龙破甲.exe"

# 直运行 EXE 时将 Skills 放在旁边，保证运行时可扫描。
cp bridge.md "$OUT_DIR/bridge.md"
cp -R codex-skills "$OUT_DIR/codex-skills"

echo "[OK] $OUT_DIR/矩龙破甲.exe"
echo "[OK] $OUT_DIR/bridge.md"
echo "[OK] $OUT_DIR/codex-skills/"
