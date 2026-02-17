# OpenISI Release Process

This guide explains how to build, package, and release OpenISI. It covers version management, the packaging pipeline, platform-specific details, and the CI workflow.

## Overview

OpenISI ships three components as a single download:

| Component | Language | Build tool | Output |
|-----------|----------|------------|--------|
| Application | GDScript | Godot export | `.app` / `.exe` / binary + `.pck` |
| Extension | Rust | Cargo | `.dylib` / `.dll` / `.so` |
| Camera daemon | Python | PyInstaller | Standalone executable + embedded runtime |

Godot's export packs resources into a `.pck` file. The daemon is a native executable that must exist on the filesystem to be launched via `OS.create_process()`. It cannot go in `.pck`. A post-export packaging script places the daemon alongside the Godot binary.

---

## Version Management

Version is tracked in three files that must stay in sync:

| File | Field | Component |
|------|-------|-----------|
| `src/autoload/version.gd` | `CURRENT` constant | Godot app |
| `pyproject.toml` | `version` field | Python daemon |
| `extension/Cargo.toml` | `version` field | Rust extension |

Bump all three with:

```bash
./scripts/bump-version.sh <version>
```

This script updates all three files, commits the change, and creates a git tag.

---

## Release Workflow

1. Update `CHANGELOG.md` with release notes under the version heading
2. Run `./scripts/bump-version.sh 0.2.0`
3. Push: `git push && git push --tags`
4. GitHub Actions builds all platforms and creates a Release with assets

---

## Local Build

For testing locally before pushing:

```bash
# macOS
./scripts/package-macos.sh

# Windows (PowerShell)
./scripts/package-windows.ps1

# Linux
./scripts/package-linux.sh
```

Output goes to `build/{platform}/`.

---

## Packaging Pipeline

Each platform script does the same steps:

1. Build Rust extension (release mode)
2. Build Python daemon via PyInstaller
3. Export Godot app via `godot --headless --export-release`
4. Copy daemon bundle alongside Godot binary
5. (macOS only) Sign the bundle
6. Create archive for GitHub Releases

---

## Platform Details

### macOS

**Bundle structure:**

```
OpenISI.app/Contents/
  MacOS/
    OpenISI                          <- Godot binary
    openisi-daemon-arm64/            <- ARM64 daemon (Apple Silicon)
      openisi-daemon
      _internal/
    openisi-daemon-x86_64/           <- Intel daemon
      openisi-daemon
      _internal/
  Frameworks/
    libopenisi_shm.dylib            <- Universal binary (ARM64 + Intel)
  Resources/
    OpenISI.pck                      <- Godot resources
```

**Universal binary:** macOS builds support both Apple Silicon and Intel. The Rust extension is built for both architectures and merged with `lipo`. The Python daemon is built separately on each architecture (PyInstaller cannot cross-compile). `PythonUtils.get_daemon_executable()` selects the correct daemon based on the running architecture.

**Code signing:** Currently ad-hoc (unsigned). Supports Apple Developer identity via `CODESIGN_IDENTITY` environment variable. Signing order (innermost first):

1. All `.dylib`/`.so` inside `openisi-daemon-*/_internal/`
2. `Python.framework` inside `_internal/`
3. `openisi-daemon-*/openisi-daemon`
4. `Frameworks/libopenisi_shm.dylib`
5. `OpenISI.app` (outermost)

**Entitlements:**

| Key | Purpose |
|-----|---------|
| `com.apple.security.device.camera` | Camera access |
| `com.apple.security.cs.disable-library-validation` | Load GDExtension |
| `com.apple.security.cs.allow-unsigned-executable-memory` | Godot runtime |
| `com.apple.security.network.client` | Update checker |

**Gatekeeper:** Without an Apple Developer certificate, users must right-click and choose Open on first launch. Document this in release notes.

### Windows

**Structure:**

```
OpenISI-windows/
  OpenISI.exe                        <- Godot binary
  OpenISI.pck                        <- Godot resources
  openisi_shm.dll                    <- Rust extension
  openisi-daemon/
    openisi-daemon.exe
    _internal/
```

Godot on Windows places `.pck` and `.dll` beside the `.exe`. No code signing currently.

### Linux

**Structure:**

```
OpenISI-linux/
  OpenISI                            <- Godot binary
  OpenISI.pck
  libopenisi_shm.so
  openisi-daemon/
    openisi-daemon
    _internal/
```

Distributed as `.tar.gz`. User extracts and runs.

---

## Asset Naming Convention

GitHub Release assets must use these exact names (the in-app update checker matches on them):

- `OpenISI-macos.zip`
- `OpenISI-windows.zip`
- `OpenISI-linux.tar.gz`

---

## CI Pipeline

GitHub Actions workflow (`.github/workflows/release.yml`) triggers on tag push (`v*`):

| Job | Runner | Output |
|-----|--------|--------|
| `build-macos-arm64` | `macos-14` | ARM64 `.app` + daemon |
| `build-macos-x86_64` | `macos-13` | Intel `.app` + daemon |
| `merge-macos` | `macos-14` | Universal `.app` with both daemons, `OpenISI-macos.zip` |
| `build-windows` | `windows-latest` | `OpenISI-windows.zip` |
| `build-linux` | `ubuntu-latest` | `OpenISI-linux.tar.gz` |
| `release` | `ubuntu-latest` | Creates GitHub Release, attaches all archives |

---

## Code Signing (Future)

When signing accounts are available:

**macOS:**

```bash
CODESIGN_IDENTITY="Developer ID Application: ..." ./scripts/package-macos.sh
```

Notarization:

```bash
xcrun notarytool submit OpenISI-macos.zip --apple-id ... --team-id ... --password ... --wait
xcrun stapler staple OpenISI.app
```

**Windows:** Add signing step to `package-windows.ps1` using `signtool.exe`.
