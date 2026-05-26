# OpenISI development environment setup for Windows.

Write-Host "=== OpenISI Setup ===" -ForegroundColor Cyan

# ── Detect legacy LIBTORCH_USE_PYTORCH ────────────────────────────────
# torch-sys switches to a Python-torch path the instant LIBTORCH_USE_PYTORCH
# is set in the environment (regardless of value), and cargo's [env] table
# can override values but cannot unset a variable. So the developer's
# environment must not have it set.
if (Test-Path env:LIBTORCH_USE_PYTORCH) {
    Write-Host ""
    Write-Host "[warning] LIBTORCH_USE_PYTORCH is set in your environment." -ForegroundColor Yellow
    Write-Host "          This forces tch to use your system Python torch instead of the"
    Write-Host "          project-vendored libtorch. Remove it from your user environment"
    Write-Host "          (System Properties -> Environment Variables, or in your PowerShell"
    Write-Host "          profile), then open a new terminal and re-run this setup."
    Write-Host ""
}

# ── Rust ──────────────────────────────────────────────────────────────
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    Write-Host "[ok] Rust installed: $(rustc --version)"
} else {
    Write-Host "[install] Rust not found - installing via rustup..."
    Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile rustup-init.exe
    .\rustup-init.exe -y
    Remove-Item rustup-init.exe
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    Write-Host "[ok] Rust installed: $(rustc --version)"
}

# ── CMake ─────────────────────────────────────────────────────────────
if (Get-Command cmake -ErrorAction SilentlyContinue) {
    Write-Host "[ok] CMake installed: $(cmake --version | Select-Object -First 1)"
} else {
    Write-Host "[install] CMake not found - installing via winget..."
    winget install --id Kitware.CMake --accept-package-agreements --accept-source-agreements
    Write-Host "[ok] CMake installed"
}

# ── libtorch ──────────────────────────────────────────────────────────
# Project-managed: we download libtorch into vendor\libtorch and point
# cargo at it via .cargo\config.toml. The build is hermetic and does not
# depend on whatever libtorch (or Python torch) happens to be installed
# system-wide. See docs\ANALYSIS_COMPUTE.md.

$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$LibtorchVersion = "2.11.0"
$LibtorchDir = Join-Path $RepoRoot "vendor\libtorch"
$LibtorchMarker = Join-Path $RepoRoot "vendor\libtorch.version"

$needsLibtorch = $true
if ((Test-Path $LibtorchMarker) -and (Test-Path (Join-Path $LibtorchDir "lib"))) {
    $installedVer = (Get-Content $LibtorchMarker).Trim()
    if ($installedVer -eq $LibtorchVersion) {
        Write-Host "[ok] libtorch $LibtorchVersion installed at vendor\libtorch"
        $needsLibtorch = $false
    } else {
        Write-Host "[install] libtorch version mismatch ($installedVer -> $LibtorchVersion), reinstalling..."
        Remove-Item -Recurse -Force $LibtorchDir
        Remove-Item -Force $LibtorchMarker
    }
}

if ($needsLibtorch) {
    # Detect CUDA — prefer CUDA build if nvidia-smi is on the path
    $cudaPresent = (Get-Command nvidia-smi -ErrorAction SilentlyContinue) -ne $null
    if ($cudaPresent) {
        Write-Host "[detect] CUDA detected - using CUDA 12.6 libtorch build"
        $libtorchUrl = "https://download.pytorch.org/libtorch/cu126/libtorch-win-shared-with-deps-$LibtorchVersion%2Bcu126.zip"
    } else {
        Write-Host "[detect] No CUDA detected - using CPU libtorch build"
        $libtorchUrl = "https://download.pytorch.org/libtorch/cpu/libtorch-win-shared-with-deps-$LibtorchVersion%2Bcpu.zip"
    }

    Write-Host "[install] Downloading libtorch $LibtorchVersion"
    Write-Host "          $libtorchUrl"
    New-Item -ItemType Directory -Force (Join-Path $RepoRoot "vendor") | Out-Null
    $tmpZip = Join-Path $RepoRoot "vendor\libtorch.zip"
    Invoke-WebRequest -Uri $libtorchUrl -OutFile $tmpZip

    Write-Host "[install] Extracting to vendor\libtorch..."
    if (Test-Path $LibtorchDir) { Remove-Item -Recurse -Force $LibtorchDir }
    Expand-Archive -Path $tmpZip -DestinationPath (Join-Path $RepoRoot "vendor") -Force
    Remove-Item -Force $tmpZip

    if (-not (Test-Path (Join-Path $LibtorchDir "lib"))) {
        Write-Host "[error] Extraction did not produce vendor\libtorch\lib - archive layout unexpected" -ForegroundColor Red
        exit 1
    }

    Set-Content -Path $LibtorchMarker -Value $LibtorchVersion
    Write-Host "[ok] libtorch $LibtorchVersion installed at vendor\libtorch"
}

# ── Tauri CLI ─────────────────────────────────────────────────────────
$tauriCheck = cargo tauri --version 2>$null
if ($LASTEXITCODE -eq 0) {
    Write-Host "[ok] Tauri CLI installed: $tauriCheck"
} else {
    Write-Host "[install] Installing Tauri CLI..."
    cargo install tauri-cli --version "^2"
    Write-Host "[ok] Tauri CLI installed: $(cargo tauri --version)"
}

# ── Verify build ──────────────────────────────────────────────────────
Write-Host ""
Write-Host "=== Verifying build ===" -ForegroundColor Cyan
# Temporarily clear LIBTORCH_USE_PYTORCH for this invocation so the verify
# succeeds even when the user hasn't yet removed it from their environment.
$savedPyTorch = $env:LIBTORCH_USE_PYTORCH
$env:LIBTORCH_USE_PYTORCH = $null
try {
    cargo check --workspace
} finally {
    if ($savedPyTorch) { $env:LIBTORCH_USE_PYTORCH = $savedPyTorch }
}

Write-Host ""
Write-Host "=== Setup complete ===" -ForegroundColor Green
Write-Host "Run the app with:  cargo tauri dev"
