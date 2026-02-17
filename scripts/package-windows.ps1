# Package OpenISI for Windows.
#
# Builds the Rust extension, Python daemon (with PCO), and Godot app,
# then assembles them into a folder and creates OpenISI-windows.zip.
#
# Usage: .\scripts\package-windows.ps1
#
# Environment variables:
#   GODOT  - Path to Godot binary (default: "godot")

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir = Split-Path -Parent $ScriptDir
$BuildDir = Join-Path $ProjectDir "build\windows"
$DistDir = Join-Path $ProjectDir "dist"

$Godot = if ($env:GODOT) { $env:GODOT } else { "godot" }

Write-Host "=========================================="
Write-Host "OpenISI Windows Packager"
Write-Host "=========================================="

# Clean previous build
if (Test-Path $BuildDir) { Remove-Item -Recurse -Force $BuildDir }
New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

# --- Step 1: Build Rust extension ---
Write-Host ""
Write-Host "--- Building Rust extension ---"
Push-Location (Join-Path $ProjectDir "extension")
cargo build --release
Pop-Location
Copy-Item (Join-Path $ProjectDir "extension\target\release\openisi_shm.dll") `
    (Join-Path $ProjectDir "bin\openisi_shm.dll")

# --- Step 2: Build Python daemon (with PCO) ---
Write-Host ""
Write-Host "--- Building Python daemon ---"
Push-Location $ProjectDir
poetry install --extras pco
Pop-Location
Push-Location (Join-Path $ProjectDir "daemon")
poetry run pyinstaller openisi-daemon.spec --distpath $DistDir --noconfirm
Pop-Location

# --- Step 3: Export Godot app ---
Write-Host ""
Write-Host "--- Exporting Godot app ---"
Push-Location $ProjectDir
& $Godot --headless --export-release "Windows Desktop" (Join-Path $BuildDir "OpenISI.exe")
Pop-Location

if (-not (Test-Path (Join-Path $BuildDir "OpenISI.exe"))) {
    Write-Host "ERROR: Godot export failed - OpenISI.exe not found"
    exit 1
}

# --- Step 4: Assemble ---
Write-Host ""
Write-Host "--- Assembling package ---"
$DaemonSrc = Join-Path $DistDir "openisi-daemon"
$DaemonDst = Join-Path $BuildDir "openisi-daemon"
Copy-Item -Recurse $DaemonSrc $DaemonDst
Write-Host "  Daemon copied to: openisi-daemon\"

# --- Step 5: Create zip ---
Write-Host ""
Write-Host "--- Creating archive ---"
$ZipPath = Join-Path $DistDir "OpenISI-windows.zip"
if (Test-Path $ZipPath) { Remove-Item $ZipPath }
Compress-Archive -Path (Join-Path $BuildDir "*") -DestinationPath $ZipPath

Write-Host ""
Write-Host "=========================================="
Write-Host "Build complete!"
Write-Host "  App:     $BuildDir\OpenISI.exe"
Write-Host "  Archive: $ZipPath"
Write-Host "=========================================="
