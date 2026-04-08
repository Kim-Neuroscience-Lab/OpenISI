#!/usr/bin/env bash
set -euo pipefail

# OpenISI development environment setup.
# Works on macOS and Linux. For Windows, use setup.ps1.

echo "=== OpenISI Setup ==="

# ── Rust ──────────────────────────────────────────────────────────────
if command -v rustc &>/dev/null; then
    RUST_VER=$(rustc --version)
    echo "[ok] Rust installed: $RUST_VER"
else
    echo "[install] Rust not found — installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "[ok] Rust installed: $(rustc --version)"
fi

# ── CMake (needed for HDF5 static build) ─────────────────────────────
if command -v cmake &>/dev/null; then
    echo "[ok] CMake installed: $(cmake --version | head -1)"
else
    echo "[install] CMake not found — installing..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
        if command -v brew &>/dev/null; then
            brew install cmake
        else
            echo "[error] Install Homebrew (https://brew.sh) first, then re-run."
            exit 1
        fi
    else
        sudo apt-get update && sudo apt-get install -y cmake
    fi
    echo "[ok] CMake installed: $(cmake --version | head -1)"
fi

# ── Platform dependencies ────────────────────────────────────────────
if [[ "$OSTYPE" == "linux"* ]]; then
    echo "[install] Installing Linux dependencies..."
    sudo apt-get update
    sudo apt-get install -y \
        libwebkit2gtk-4.1-dev \
        libappindicator3-dev \
        librsvg2-dev \
        patchelf
    echo "[ok] Linux dependencies installed"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    echo "[ok] macOS — no additional system dependencies needed"
fi

# ── Tauri CLI ────────────────────────────────────────────────────────
if cargo tauri --version &>/dev/null; then
    echo "[ok] Tauri CLI installed: $(cargo tauri --version)"
else
    echo "[install] Installing Tauri CLI..."
    cargo install tauri-cli --version "^2"
    echo "[ok] Tauri CLI installed: $(cargo tauri --version)"
fi

# ── Verify build ─────────────────────────────────────────────────────
echo ""
echo "=== Verifying build ==="
cargo check --workspace
echo ""
echo "=== Setup complete ==="
echo "Run the app with:  cargo tauri dev"
