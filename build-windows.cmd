@echo off
setlocal
if not exist "%~dp0instruction-packs\gpt-5.6-sol-v45.md" (
  echo Missing instruction pack resource: gpt-5.6-sol-v45.md
  exit /b 1
)
if not exist "%~dp0instruction-packs\gpt-6-astra-v1.md" (
  echo Missing instruction pack resource: gpt-6-astra-v1.md
  exit /b 1
)
if not exist "%~dp0instruction-lab\release-catalog.json" (
  echo Missing instruction lab resource: release-catalog.json
  exit /b 1
)
if not exist "%~dp0instruction-packs\gpt-6-astra-v2-rc1.md" (
  echo Missing instruction resource: instruction-packs/gpt-6-astra-v2-rc1.md
  exit /b 1
)
if not exist "%~dp0instruction-packs\gpt-6.1-sol-v1-rc2.md" (
  echo Missing instruction resource: instruction-packs/gpt-6.1-sol-v1-rc2.md
  exit /b 1
)
if not exist "%~dp0instruction-lab\legacy\release-catalog.json" (
  echo Missing instruction resource: instruction-lab/legacy/release-catalog.json
  exit /b 1
)
if not exist "%~dp0instruction-lab\legacy\issue-regression.jsonl" (
  echo Missing instruction resource: instruction-lab/legacy/issue-regression.jsonl
  exit /b 1
)
if not exist "%~dp0instruction-lab\upstream-manifest.json" (
  echo Missing instruction resource: instruction-lab/upstream-manifest.json
  exit /b 1
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1" %*
exit /b %ERRORLEVEL%
