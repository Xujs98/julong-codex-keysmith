param(
    [ValidateSet("exe", "nsis", "msi", "all")]
    [string]$Mode = "nsis",

    [ValidateSet("x64", "arm64")]
    [string]$Arch = "x64"
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ProjectRoot

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Write-Host "Windows bundles must be built on Windows." -ForegroundColor Red
    exit 1
}

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        Write-Host "Missing required command: $Name" -ForegroundColor Red
        exit 1
    }
}

Require-Command "node"
Require-Command "npm"
Require-Command "cargo"
Require-Command "rustup"

$Target = if ($Arch -eq "arm64") {
    "aarch64-pc-windows-msvc"
} else {
    "x86_64-pc-windows-msvc"
}

$Bundles = switch ($Mode) {
    "exe"  { "" }
    "nsis" { "nsis" }
    "msi"  { "msi" }
    default { "nsis,msi" }
}

Write-Host "=== Windows Release Build ===" -ForegroundColor Cyan
Write-Host "Target : $Target"
Write-Host "Bundles: $Bundles"

Write-Host "[1/4] Installing JavaScript dependencies..." -ForegroundColor Yellow
if (Test-Path "package-lock.json") {
    & npm ci
} else {
    & npm install
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[2/4] Installing Rust target..." -ForegroundColor Yellow
& rustup target add $Target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[3/4] Building Tauri application..." -ForegroundColor Yellow
if ($Mode -eq "exe") {
    & npm exec tauri -- build --target $Target --no-bundle
} else {
    & npm exec tauri -- build --target $Target --bundles $Bundles
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$ReleaseDir = Join-Path $ProjectRoot "src-tauri\target\$Target\release"
$BundleDir = Join-Path $ReleaseDir "bundle"
Write-Host "[4/4] Build artifacts:" -ForegroundColor Yellow
if (Test-Path $ReleaseDir) {
    Get-ChildItem $ReleaseDir -Recurse -File |
        Where-Object { $_.Extension -in ".exe", ".msi" } |
        ForEach-Object { Write-Host "[OK] $($_.FullName)" -ForegroundColor Green }
} else {
    Write-Host "Bundle directory not found: $BundleDir" -ForegroundColor Red
    exit 1
}

Write-Host "=== Build complete ===" -ForegroundColor Cyan
