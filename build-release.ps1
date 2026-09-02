# Backward-compatible NSIS build entry.
& "$PSScriptRoot\build-windows.ps1" -Mode nsis -Arch x64
exit $LASTEXITCODE
