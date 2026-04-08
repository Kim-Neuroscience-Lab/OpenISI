# OpenISI development environment setup for Windows.

Write-Host "=== OpenISI Setup ===" -ForegroundColor Cyan

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
cargo check --workspace

Write-Host ""
Write-Host "=== Setup complete ===" -ForegroundColor Green
Write-Host "Run the app with:  cargo tauri dev"
