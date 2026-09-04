#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="${1:-$ROOT/src-tauri/target/debug/julong-codex}"

if [[ ! -x "$CLI" ]]; then
  echo "CLI not found: $CLI"
  echo "Build it with: cargo build --manifest-path src-tauri/Cargo.toml --bin julong-codex"
  exit 2
fi

if python3 - <<'PY'
import socket
s = socket.socket()
s.settimeout(.3)
raise SystemExit(0 if s.connect_ex(('127.0.0.1', 8080)) == 0 else 1)
PY
then
  echo "SKIP: 127.0.0.1:8080 is already in use; existing proxy was left untouched."
  exit 77
fi

TMP_HOME="$(mktemp -d "${TMPDIR:-/tmp}/julong-cli-verify.XXXXXX")"
cleanup() {
  CODEX_HOME="$TMP_HOME" "$CLI" stop >/dev/null 2>&1 || true
  rm -rf "$TMP_HOME"
}
trap cleanup EXIT INT TERM

cat > "$TMP_HOME/config.toml" <<'EOF'
model = "verification-model"
model_provider = "verification"

[model_providers.verification]
name = "Verification"
base_url = "http://127.0.0.1:19080/v1"
EOF

run_step() {
  local name="$1"
  shift
  echo "=== $name ==="
  set +e
  CODEX_HOME="$TMP_HOME" "$CLI" "$@"
  local code=$?
  set -e
  echo "exit_code=$code"
  [[ $code -eq 0 ]]
}

run_step status-before status
run_step start-first start
run_step start-idempotent start
run_step status-running status
run_step stop-first stop
run_step stop-idempotent stop
run_step status-after status

grep -q '127.0.0.1:19080/v1' "$TMP_HOME/config.toml"
! grep -q '127.0.0.1:8080' "$TMP_HOME/config.toml"
echo "[OK] CLI start/stop/status idempotency and configuration rollback verified."
