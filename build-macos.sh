#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-all}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS bundles must be built on macOS."
  exit 1
fi

for command_name in node npm cargo rustup xcrun; do
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "Missing required command: ${command_name}"
    exit 1
  fi
done

case "${MODE}" in
  intel)
    TARGETS=("x86_64-apple-darwin")
    SCRIPTS=("build:mac:intel")
    ;;
  apple|arm64)
    TARGETS=("aarch64-apple-darwin")
    SCRIPTS=("build:mac:apple")
    ;;
  universal)
    TARGETS=("x86_64-apple-darwin" "aarch64-apple-darwin")
    SCRIPTS=("build:mac:universal")
    ;;
  all)
    TARGETS=("x86_64-apple-darwin" "aarch64-apple-darwin")
    SCRIPTS=("build:mac:intel" "build:mac:apple")
    ;;
  *)
    echo "Usage: $0 [intel|apple|universal|all]"
    exit 2
    ;;
esac

echo "Installing JavaScript dependencies..."
if [[ -f package-lock.json ]]; then
  npm ci
else
  npm install
fi

echo "Installing Rust targets: ${TARGETS[*]}"
rustup target add "${TARGETS[@]}"

for script_name in "${SCRIPTS[@]}"; do
  echo "Running npm script: ${script_name}"
  npm run "${script_name}"
done

echo "Build artifacts:"
find src-tauri/target -path "*/release/bundle/*" -type f \
  \( -name "*.app.tar.gz" -o -name "*.dmg" -o -name "Info.plist" \) -print
