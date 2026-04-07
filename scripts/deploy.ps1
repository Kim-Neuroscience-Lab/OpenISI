# Deploy OpenISI to C:\Program Files\OpenISI with desktop shortcut.
# Run from the openisi-rust root: powershell -ExecutionPolicy Bypass -File scripts\deploy.ps1

$ErrorActionPreference = "Stop"

$SrcDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (!(Test-Path "$SrcDir\openisi-rust")) { $SrcDir = Split-Path -Parent $PSScriptRoot }

$InstallDir = "C:\Program Files\OpenISI"
$DesktopDir = "C:\Users\ISI User\Desktop"
$ExeSrc = "$SrcDir\target\release\openisi.exe"
$HeadlessSrc = "$SrcDir\target\release\headless.exe"
$ConfigSrc = "$SrcDir\config"

# Build release.
Write-Host "Building release..." -ForegroundColor Cyan
$CargoPath = "C:\Users\ISI User\.cargo\bin\cargo.exe"
& $CargoPath build --release
if ($LASTEXITCODE -ne 0) { throw "Build failed" }

# Create install directory.
Write-Host "Installing to $InstallDir..." -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path "$InstallDir\config" | Out-Null

# Copy files.
Copy-Item $ExeSrc "$InstallDir\openisi.exe" -Force
if (Test-Path $HeadlessSrc) { Copy-Item $HeadlessSrc "$InstallDir\headless.exe" -Force }
Copy-Item "$ConfigSrc\*" "$InstallDir\config\" -Force -Recurse

# Copy icon.
if (Test-Path "$SrcDir\src-tauri\icons\icon.ico") {
    Copy-Item "$SrcDir\src-tauri\icons\icon.ico" "$InstallDir\icon.ico" -Force
}

# Create desktop shortcut.
Write-Host "Creating desktop shortcut..." -ForegroundColor Cyan
$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut("$DesktopDir\OpenISI.lnk")
$Shortcut.TargetPath = "$InstallDir\openisi.exe"
$Shortcut.WorkingDirectory = $InstallDir
$Shortcut.Description = "OpenISI - Intrinsic Signal Imaging"
if (Test-Path "$InstallDir\icon.ico") {
    $Shortcut.IconLocation = "$InstallDir\icon.ico"
}
$Shortcut.Save()

Write-Host "Done! OpenISI installed to $InstallDir" -ForegroundColor Green
Write-Host "Desktop shortcut created at $DesktopDir\OpenISI.lnk" -ForegroundColor Green
