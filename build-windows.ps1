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

Write-Host "[1/5] Installing JavaScript dependencies..." -ForegroundColor Yellow
if (Test-Path "package-lock.json") {
    & npm ci
} else {
    & npm install
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[2/5] Installing Rust target..." -ForegroundColor Yellow
& rustup target add $Target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$BinariesDir = Join-Path $ProjectRoot "src-tauri\binaries"
New-Item -ItemType Directory -Path $BinariesDir -Force | Out-Null
$CliSidecar = Join-Path $BinariesDir "julong-codex-$Target.exe"
if (-not (Test-Path $CliSidecar)) {
    New-Item -ItemType File -Path $CliSidecar -Force | Out-Null
}

Write-Host "[3/5] Building julong-codex CLI sidecar..." -ForegroundColor Yellow
& cargo build --manifest-path "src-tauri/Cargo.toml" --bin julong-codex --target $Target --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$CliSource = Join-Path $ProjectRoot "src-tauri\target\$Target\release\julong-codex.exe"
Copy-Item $CliSource $CliSidecar -Force
Write-Host "[OK] $CliSidecar" -ForegroundColor Green

Write-Host "[4/5] Building Tauri application..." -ForegroundColor Yellow
if ($Mode -eq "exe") {
    & npm exec tauri -- build --config "src-tauri/tauri.sidecar.conf.json" --target $Target --no-bundle
} else {
    & npm exec tauri -- build --config "src-tauri/tauri.sidecar.conf.json" --target $Target --bundles $Bundles
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$ReleaseDir = Join-Path $ProjectRoot "src-tauri\target\$Target\release"
$BundleDir = Join-Path $ReleaseDir "bundle"
Write-Host "[5/5] Build artifacts:" -ForegroundColor Yellow
if (Test-Path $ReleaseDir) {
    Get-ChildItem $ReleaseDir -Recurse -File |
        Where-Object { $_.Extension -in ".exe", ".msi" } |
        ForEach-Object { Write-Host "[OK] $($_.FullName)" -ForegroundColor Green }
} else {
    Write-Host "Bundle directory not found: $BundleDir" -ForegroundColor Red
    exit 1
}

Write-Host "=== Build complete ===" -ForegroundColor Cyan
