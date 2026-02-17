# OpenISI Developer Build Guide

This guide walks you through setting up a local development environment for OpenISI. It covers prerequisites, building each component, and running the app from source.

---

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Godot | 4.6 | Game engine / app runtime |
| Rust | stable | GDExtension (shared memory, monitor detection) |
| Python | 3.13+ | Camera daemon |
| Poetry | 2.x | Python dependency management |

### Platform-Specific Requirements

**macOS:**
```bash
xcode-select --install
```
Xcode Command Line Tools provide the C compiler and system headers needed by Rust and Python native extensions.

**Windows:**
Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **C++ desktop development** workload. This is required for Rust compilation.

**Linux:**
```bash
sudo apt install build-essential pkg-config libvulkan-dev
```

---

## Clone & Setup

```bash
git clone https://github.com/Kim-Neuroscience-Lab/OpenISI.git
cd OpenISI

# Python dependencies
poetry install

# Rust extension
cd extension
cargo build
cd ..
```

---

## Running in Development

Open the project in the Godot 4.6 editor and press **F5** (or the Play button). In dev mode:

- The Python daemon runs from the virtual environment (`.venv/`)
- The Rust extension loads from `bin/`
- Camera enumeration uses `python -m daemon.camera.enumerate`

---

## Project Structure

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed architecture documentation.

```
openisi/
├── src/           → Godot application (GDScript)
├── daemon/        → Python camera daemon
├── extension/     → Rust GDExtension
├── bin/           → Compiled extension binaries
├── docs/          → Documentation
├── scripts/       → Build and release scripts
├── config/        → Default configuration files
└── hardware/      → Hardware guides (photodiode, etc.)
```

---

## Building the Rust Extension

The Rust extension (`openisi_shm`) provides shared memory IPC and monitor detection via GDExtension.

```bash
cd extension
cargo build --release
```

Copy the compiled library into `bin/` so Godot can load it:

```bash
# macOS
cp target/release/libopenisi_shm.dylib ../bin/

# Windows
# cp target/release/openisi_shm.dll ../bin/

# Linux
# cp target/release/libopenisi_shm.so ../bin/
```

---

## Building the Python Daemon

For testing the bundled daemon locally:

```bash
poetry run pyinstaller daemon/openisi-daemon.spec --distpath dist --noconfirm
```

The bundled daemon at `dist/openisi-daemon/` includes the Python runtime and all dependencies. This is the form shipped with release builds.

---

## Building for Distribution

See [RELEASING.md](RELEASING.md) for the full packaging and release process.

---

## Cross-Platform Notes

- **macOS (ARM64 + Intel):** The release build creates a universal binary. For local development, building for your native architecture is sufficient.
- **Windows:** The pco camera SDK is available on Windows. Install with `poetry install --extras pco`.
- **Linux:** The pco camera SDK is available on Linux. Install with `poetry install --extras pco`. Ensure `libvulkan-dev` is installed for display support.

---

## Testing

Currently manual testing. Checklist:

1. Godot editor runs without errors
2. Camera enumeration detects available cameras
3. Display validation measures correct refresh rate
4. Live preview shows camera feed
5. Stimulus preview renders correctly
6. Full acquisition completes without dropped frames
