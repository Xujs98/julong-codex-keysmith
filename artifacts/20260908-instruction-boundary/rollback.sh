#!/bin/sh
set -eu
rollback_paths='README.md VERSION frontend/app.js frontend/index.html frontend/styles.css inkos.json package-lock.json package.json src-tauri/Cargo.lock src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/src/cli.rs src-tauri/src/deploy.rs src-tauri/src/lib.rs'
if [ -n "$(git status --porcelain -- $rollback_paths)" ]; then
  echo 'working tree has additional changes in rollback paths; aborting'
  exit 2
fi
git restore --source=HEAD -- $rollback_paths
python3 - <<'PY'
from pathlib import Path
for name in ('src-tauri/src/adapters.rs', 'src-tauri/src/instruction.rs'):
    path = Path(name)
    if path.exists():
        path.unlink()
PY
printf '%s\n' '[OK] instruction-boundary rollback complete'
