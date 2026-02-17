#!/usr/bin/env bash
set -euo pipefail

# Package OpenISI for macOS.
#
# Builds the Rust extension, Python daemon, and Godot app, then assembles
# them into a signed .app bundle and creates OpenISI-macos.zip.
#
# Usage: ./scripts/package-macos.sh
#
# Environment variables:
#   CODESIGN_IDENTITY  - Code signing identity (default: ad-hoc "-")
#   GODOT              - Path to Godot binary (default: "godot")

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build/macos"
DIST_DIR="$PROJECT_DIR/dist"

CODESIGN_IDENTITY="${CODESIGN_IDENTITY:--}"
GODOT="${GODOT:-godot}"

echo "=========================================="
echo "OpenISI macOS Packager"
echo "=========================================="

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
    arm64)  RUST_TARGET="aarch64-apple-darwin"; DAEMON_ARCH="arm64" ;;
    x86_64) RUST_TARGET="x86_64-apple-darwin"; DAEMON_ARCH="x86_64" ;;
    *)      echo "ERROR: Unsupported architecture: $ARCH"; exit 1 ;;
esac
echo "Architecture: $ARCH ($RUST_TARGET)"

# Clean previous build
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR" "$DIST_DIR"

# --- Step 1: Build Rust extension ---
echo ""
echo "--- Building Rust extension ($RUST_TARGET) ---"
cd "$PROJECT_DIR/extension"
cargo build --release --target "$RUST_TARGET"
cp "target/$RUST_TARGET/release/libopenisi_shm.dylib" "$PROJECT_DIR/bin/libopenisi_shm.dylib"

# --- Step 2: Build Python daemon ---
echo ""
echo "--- Building Python daemon ---"
cd "$PROJECT_DIR"
poetry install
cd "$PROJECT_DIR/daemon"
poetry run pyinstaller openisi-daemon.spec --distpath "$DIST_DIR" --noconfirm

# --- Step 3: Export Godot app ---
echo ""
echo "--- Exporting Godot app ---"
cd "$PROJECT_DIR"
mkdir -p "$BUILD_DIR"
"$GODOT" --headless --export-release "macOS" "$BUILD_DIR/OpenISI.app"

# Verify export produced the .app
if [[ ! -d "$BUILD_DIR/OpenISI.app" ]]; then
    echo "ERROR: Godot export failed - OpenISI.app not found"
    exit 1
fi

# --- Step 4: Assemble bundle ---
echo ""
echo "--- Assembling .app bundle ---"
MACOS_DIR="$BUILD_DIR/OpenISI.app/Contents/MacOS"

# Copy daemon into .app
DAEMON_DIR="$MACOS_DIR/openisi-daemon-$DAEMON_ARCH"
cp -R "$DIST_DIR/openisi-daemon" "$DAEMON_DIR"
echo "  Daemon copied to: openisi-daemon-$DAEMON_ARCH/"

# Verify GDExtension
if [[ -d "$BUILD_DIR/OpenISI.app/Contents/Frameworks" ]]; then
    echo "  GDExtension found in Contents/Frameworks/"
else
    echo "  WARNING: Contents/Frameworks/ not found - GDExtension may be in MacOS/"
fi

# --- Step 5: Code sign ---
echo ""
echo "--- Code signing (identity: $CODESIGN_IDENTITY) ---"

# Entitlements file
ENTITLEMENTS="$BUILD_DIR/entitlements.plist"
cat > "$ENTITLEMENTS" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.device.camera</key>
    <true/>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
    <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
    <true/>
    <key>com.apple.security.network.client</key>
    <true/>
</dict>
</plist>
PLIST

# Sign innermost first: daemon _internal libs, daemon exe, then .app
echo "  Signing daemon libraries..."
find "$DAEMON_DIR/_internal" -type f \( -name "*.so" -o -name "*.dylib" \) -exec \
    codesign --force --sign "$CODESIGN_IDENTITY" --timestamp {} \; 2>/dev/null || true

echo "  Signing daemon executable..."
codesign --force --sign "$CODESIGN_IDENTITY" --timestamp "$DAEMON_DIR/openisi-daemon"

echo "  Signing GDExtension..."
find "$BUILD_DIR/OpenISI.app" -name "*.dylib" -not -path "*/openisi-daemon*" -exec \
    codesign --force --sign "$CODESIGN_IDENTITY" --timestamp {} \; 2>/dev/null || true

echo "  Signing .app bundle..."
codesign --force --deep --sign "$CODESIGN_IDENTITY" --entitlements "$ENTITLEMENTS" --timestamp "$BUILD_DIR/OpenISI.app"

# Verify
echo "  Verifying signature..."
codesign --verify --deep "$BUILD_DIR/OpenISI.app" && echo "  Signature valid" || echo "  WARNING: Signature verification failed"

# Clean up entitlements
rm -f "$ENTITLEMENTS"

# --- Step 6: Create zip ---
echo ""
echo "--- Creating archive ---"
cd "$BUILD_DIR"
zip -r -y "$DIST_DIR/OpenISI-macos.zip" "OpenISI.app"

echo ""
echo "=========================================="
echo "Build complete!"
echo "  App:     $BUILD_DIR/OpenISI.app"
echo "  Archive: $DIST_DIR/OpenISI-macos.zip"
echo "=========================================="
