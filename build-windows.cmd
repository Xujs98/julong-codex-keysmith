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
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1" %*
exit /b %ERRORLEVEL%
