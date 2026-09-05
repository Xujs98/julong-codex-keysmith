#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-all}"

echo "=== macOS build (${MODE}) ==="
echo "This script only builds Apple targets; it does not use the Windows toolchain."

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS bundles must be built on macOS."
  exit 1
fi

for command_name in node npm cargo rustup xcrun codesign security; do
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "Missing required command: ${command_name}"
    exit 1
  fi
done

if [[ "${MODE}" == "universal" ]] && ! command -v lipo >/dev/null 2>&1; then
  echo "Missing required command: lipo"
  exit 1
fi

case "${MODE}" in
  intel)
    TARGETS=("x86_64-apple-darwin")
    SCRIPTS=("build:mac:intel")
    BUNDLE_TARGETS=("x86_64-apple-darwin")
    ;;
  apple|arm64)
    TARGETS=("aarch64-apple-darwin")
    SCRIPTS=("build:mac:apple")
    BUNDLE_TARGETS=("aarch64-apple-darwin")
    ;;
  universal)
    TARGETS=("x86_64-apple-darwin" "aarch64-apple-darwin")
    SCRIPTS=("build:mac:universal")
    BUNDLE_TARGETS=("universal-apple-darwin")
    ;;
  all)
    TARGETS=("x86_64-apple-darwin" "aarch64-apple-darwin")
    SCRIPTS=("build:mac:intel" "build:mac:apple")
    BUNDLE_TARGETS=("x86_64-apple-darwin" "aarch64-apple-darwin")
    ;;
  *)
    echo "Usage: $0 [intel|apple|universal|all]"
    exit 2
    ;;
esac

if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  APPLE_SIGNING_IDENTITY="$(
    security find-identity -v -p codesigning 2>/dev/null \
      | awk '/"Developer ID Application:/ && !found {
          line = $0
          sub(/^[^"]*"/, "", line)
          sub(/".*$/, "", line)
          print line
          found = 1
        }'
  )"
  if [[ -z "${APPLE_SIGNING_IDENTITY}" ]]; then
    APPLE_SIGNING_IDENTITY="-"
  fi
fi
export APPLE_SIGNING_IDENTITY

if [[ "${APPLE_SIGNING_IDENTITY}" == "-" ]]; then
  echo "Signing: complete ad-hoc bundle signature (local testing)"
else
  echo "Signing identity: ${APPLE_SIGNING_IDENTITY}"
fi

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

echo "Verifying macOS app signatures..."
verified_apps=0
for target in "${BUNDLE_TARGETS[@]}"; do
  app_root="src-tauri/target/${target}/release/bundle/macos"
  while IFS= read -r -d '' app_path; do
    codesign --verify --deep --strict --verbose=2 "${app_path}"
    echo "Verified: ${app_path}"
    verified_apps=$((verified_apps + 1))
  done < <(find "${app_root}" -maxdepth 1 -type d -name "*.app" -print0 2>/dev/null)
done

if [[ "${verified_apps}" -eq 0 ]]; then
  echo "No macOS app bundle was found after the build."
  exit 1
fi

echo "Build artifacts:"
for target in "${BUNDLE_TARGETS[@]}"; do
  bundle_root="src-tauri/target/${target}/release/bundle"
  find "${bundle_root}" -type f \
    \( -name "*.app.tar.gz" -o -name "*.dmg" -o -name "Info.plist" \) \
    -print 2>/dev/null || true
done
