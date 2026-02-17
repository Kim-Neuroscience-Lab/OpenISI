#!/usr/bin/env bash
set -euo pipefail

# Package OpenISI for Linux.
#
# Builds the Rust extension, Python daemon (with PCO), and Godot app,
# then assembles them into a folder and creates OpenISI-linux.tar.gz.
#
# Usage: ./scripts/package-linux.sh
#
# Environment variables:
#   GODOT  - Path to Godot binary (default: "godot")

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build/linux"
DIST_DIR="$PROJECT_DIR/dist"

GODOT="${GODOT:-godot}"

echo "=========================================="
echo "OpenISI Linux Packager"
echo "=========================================="

# Clean previous build
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR" "$DIST_DIR"

# --- Step 1: Build Rust extension ---
echo ""
echo "--- Building Rust extension ---"
cd "$PROJECT_DIR/extension"
cargo build --release
cp "target/release/libopenisi_shm.so" "$PROJECT_DIR/bin/libopenisi_shm.so"

# --- Step 2: Build Python daemon (with PCO) ---
echo ""
echo "--- Building Python daemon ---"
cd "$PROJECT_DIR"
poetry install --extras pco
cd "$PROJECT_DIR/daemon"
poetry run pyinstaller openisi-daemon.spec --distpath "$DIST_DIR" --noconfirm

# --- Step 3: Export Godot app ---
echo ""
echo "--- Exporting Godot app ---"
cd "$PROJECT_DIR"
"$GODOT" --headless --export-release "Linux" "$BUILD_DIR/OpenISI"

if [[ ! -f "$BUILD_DIR/OpenISI" ]]; then
    echo "ERROR: Godot export failed - OpenISI binary not found"
    exit 1
fi

# --- Step 4: Assemble ---
echo ""
echo "--- Assembling package ---"
cp -R "$DIST_DIR/openisi-daemon" "$BUILD_DIR/openisi-daemon"
echo "  Daemon copied to: openisi-daemon/"

# --- Step 5: Create tarball ---
echo ""
echo "--- Creating archive ---"
tar czf "$DIST_DIR/OpenISI-linux.tar.gz" -C "$BUILD_DIR" .

echo ""
echo "=========================================="
echo "Build complete!"
echo "  App:     $BUILD_DIR/OpenISI"
echo "  Archive: $DIST_DIR/OpenISI-linux.tar.gz"
echo "=========================================="
