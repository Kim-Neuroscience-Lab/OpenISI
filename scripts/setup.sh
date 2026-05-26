#!/usr/bin/env bash
set -euo pipefail

# OpenISI development environment setup.
# Works on macOS and Linux. For Windows, use setup.ps1.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== OpenISI Setup ==="

# ── Detect legacy LIBTORCH_USE_PYTORCH ────────────────────────────────
# torch-sys switches to a Python-torch path the instant LIBTORCH_USE_PYTORCH
# is *set* in the environment (regardless of value), and cargo's [env] table
# in .cargo/config.toml can override values but cannot unset a variable.
# So the developer's shell must not have it set.
if [ -n "${LIBTORCH_USE_PYTORCH+x}" ]; then
    echo ""
    echo "[warning] LIBTORCH_USE_PYTORCH is set in your shell environment (=\"$LIBTORCH_USE_PYTORCH\")."
    echo "          This forces tch to use your system Python torch instead of the"
    echo "          project-vendored libtorch. Please remove it from your shell rc:"
    echo ""
    echo "              # In ~/.zshrc or ~/.bashrc, delete the line:"
    echo "              # export LIBTORCH_USE_PYTORCH=1"
    echo ""
    echo "          Then open a new terminal (or run 'unset LIBTORCH_USE_PYTORCH') and"
    echo "          re-run this setup. Setup will continue using a temporary unset"
    echo "          for the verify step so you can complete installation, but"
    echo "          subsequent 'cargo build' invocations need the shell-side fix."
    echo ""
fi

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
        patchelf \
        unzip
    echo "[ok] Linux dependencies installed"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    echo "[ok] macOS — no additional system dependencies needed"
fi

# ── libtorch ─────────────────────────────────────────────────────────
# Project-managed: we download libtorch into vendor/libtorch and point
# cargo at it via .cargo/config.toml. The build is hermetic and does not
# depend on whatever libtorch (or Python torch) happens to be installed
# system-wide. See docs/ANALYSIS_COMPUTE.md.

LIBTORCH_VERSION="2.11.0"
LIBTORCH_DIR="$REPO_ROOT/vendor/libtorch"
LIBTORCH_MARKER="$REPO_ROOT/vendor/libtorch.version"

needs_libtorch=1
if [ -f "$LIBTORCH_MARKER" ] && [ -d "$LIBTORCH_DIR/lib" ]; then
    installed_ver=$(cat "$LIBTORCH_MARKER")
    if [[ "$installed_ver" == "$LIBTORCH_VERSION" ]]; then
        echo "[ok] libtorch $LIBTORCH_VERSION installed at vendor/libtorch"
        needs_libtorch=0
    else
        echo "[install] libtorch version mismatch ($installed_ver → $LIBTORCH_VERSION), reinstalling..."
        rm -rf "$LIBTORCH_DIR" "$LIBTORCH_MARKER"
    fi
fi

if [ "$needs_libtorch" = "1" ]; then
    arch=$(uname -m)
    libtorch_url=""

    if [[ "$OSTYPE" == "darwin"* ]]; then
        if [[ "$arch" == "arm64" ]]; then
            # Apple Silicon — CPU build includes MPS support
            libtorch_url="https://download.pytorch.org/libtorch/cpu/libtorch-macos-arm64-${LIBTORCH_VERSION}.zip"
        else
            echo "[error] Intel macOS is not supported by libtorch $LIBTORCH_VERSION."
            echo "        PyTorch dropped Intel macOS support starting at 2.3.0."
            echo "        Intel macOS development is not currently supported by this setup."
            exit 1
        fi
    elif [[ "$OSTYPE" == "linux"* ]]; then
        # Detect CUDA — prefer CUDA build if a CUDA toolkit or driver is present
        if command -v nvcc &>/dev/null || command -v nvidia-smi &>/dev/null || [ -d "/usr/local/cuda" ]; then
            echo "[detect] CUDA detected — using CUDA 12.6 libtorch build"
            libtorch_url="https://download.pytorch.org/libtorch/cu126/libtorch-shared-with-deps-${LIBTORCH_VERSION}%2Bcu126.zip"
        else
            echo "[detect] No CUDA detected — using CPU libtorch build"
            libtorch_url="https://download.pytorch.org/libtorch/cpu/libtorch-shared-with-deps-${LIBTORCH_VERSION}%2Bcpu.zip"
        fi
    else
        echo "[error] Unsupported OS: $OSTYPE"
        exit 1
    fi

    echo "[install] Downloading libtorch $LIBTORCH_VERSION"
    echo "          $libtorch_url"
    mkdir -p "$REPO_ROOT/vendor"
    tmp_zip="$REPO_ROOT/vendor/libtorch.zip"
    curl -fSL --progress-bar "$libtorch_url" -o "$tmp_zip"

    echo "[install] Extracting to vendor/libtorch..."
    rm -rf "$LIBTORCH_DIR"
    unzip -q "$tmp_zip" -d "$REPO_ROOT/vendor/"
    rm "$tmp_zip"

    if [ ! -d "$LIBTORCH_DIR/lib" ]; then
        echo "[error] Extraction did not produce vendor/libtorch/lib — archive layout unexpected"
        exit 1
    fi

    echo "$LIBTORCH_VERSION" > "$LIBTORCH_MARKER"
    echo "[ok] libtorch $LIBTORCH_VERSION installed at vendor/libtorch"
fi

# ── Make vendor/libtorch self-contained on macOS ─────────────────────
# The macOS libtorch builds reference libomp via an absolute install name
# (e.g. /opt/llvm-openmp/lib/libomp.dylib) that won't exist on a machine
# without Homebrew's libomp at that path. The bundled libomp.dylib is
# already in vendor/libtorch/lib, so we rewrite the references to
# @rpath/libomp.dylib — combined with the LC_RPATH baked into our
# binaries by src-tauri/build.rs, dyld resolves them via the vendored
# copy with no DYLD_LIBRARY_PATH or system libomp needed.
#
# Idempotent: install_name_tool on an already-@rpath name is a no-op.
# Always run so a stale install gets re-fixed if a future libtorch ships
# a different bad reference.
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "[install] Rewriting absolute libtorch install names → @rpath ..."
    # 1. Fix libomp.dylib's own LC_ID_DYLIB.
    if [ -f "$LIBTORCH_DIR/lib/libomp.dylib" ]; then
        current_id=$(otool -D "$LIBTORCH_DIR/lib/libomp.dylib" 2>/dev/null | tail -n1)
        if [[ "$current_id" != "@rpath/libomp.dylib" ]]; then
            install_name_tool -id "@rpath/libomp.dylib" "$LIBTORCH_DIR/lib/libomp.dylib"
            echo "[install]   libomp.dylib LC_ID_DYLIB: $current_id → @rpath/libomp.dylib"
        fi
    fi
    # 2. Rewrite any absolute libomp* references in every dylib.
    fixed_dylibs=()
    for dylib in "$LIBTORCH_DIR"/lib/*.dylib; do
        bad_refs=$(otool -L "$dylib" 2>/dev/null | awk '
            NR>1 && $1 ~ /libomp[^[:space:]]*\.dylib$/ && $1 ~ /^\// { print $1 }
        ')
        for bad in $bad_refs; do
            install_name_tool -change "$bad" "@rpath/libomp.dylib" "$dylib"
            echo "[install]   $(basename "$dylib"): $bad → @rpath/libomp.dylib"
            fixed_dylibs+=("$dylib")
        done
    done

    # 2b. install_name_tool produces a "linker-signed adhoc" signature
    # (flags=0x20002) that macOS library validation silently refuses to
    # load on Apple Silicon — dyld hangs the process during library load
    # with no diagnostic. A fresh `codesign --force --sign -` produces a
    # plain adhoc signature (flags=0x2) that loads cleanly. Re-sign every
    # dylib we touched, plus libomp.dylib (which had its LC_ID_DYLIB
    # rewritten above).
    resign_targets=("${fixed_dylibs[@]}")
    if [ -f "$LIBTORCH_DIR/lib/libomp.dylib" ]; then
        resign_targets+=("$LIBTORCH_DIR/lib/libomp.dylib")
    fi
    # Dedupe.
    if [ ${#resign_targets[@]} -gt 0 ]; then
        printf '%s\n' "${resign_targets[@]}" | sort -u | while read -r t; do
            codesign --force --sign - "$t" 2>&1 | sed 's/^/[install]   /'
        done
    fi

    # 3. Audit: there must be no remaining non-system absolute dylib
    # references in vendor/libtorch/lib. Any survivor will dyld-fail at
    # runtime; surface it now rather than producing a silently-broken
    # binary.
    remaining=$(for d in "$LIBTORCH_DIR"/lib/*.dylib; do
        otool -L "$d" 2>/dev/null | awk -v f="$(basename "$d")" '
            NR>1 && $1 ~ /^\// && $1 !~ /^\/usr\/lib/ && $1 !~ /^\/System\// {
                print f " → " $1
            }
        '
    done)
    if [ -n "$remaining" ]; then
        echo "[error] vendor/libtorch still has absolute non-system dylib refs:"
        echo "$remaining" | sed 's/^/        /'
        echo "        Cannot produce a self-contained binary. Aborting setup."
        exit 1
    fi
    echo "[ok] vendor/libtorch is self-contained (no absolute non-system refs)"
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
# Unset LIBTORCH_USE_PYTORCH for this invocation so the verify succeeds
# even when the user hasn't yet cleaned their shell rc (we warned above).
env -u LIBTORCH_USE_PYTORCH cargo check --workspace
echo ""
echo "=== Setup complete ==="
echo "Run the app with:  cargo tauri dev"
