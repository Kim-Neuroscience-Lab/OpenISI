# Distribution & Update Architecture

This document explains how OpenISI is packaged, distributed, and updated. It covers the bundle structure, daemon discovery, and the in-app self-update mechanism.

For release procedures (tagging, building, uploading), see `RELEASING.md`.

---

## Self-Contained Bundle

OpenISI ships as a single download with no external dependencies. Three language runtimes are bundled:

| Component | Runtime | Bundling method |
|-----------|---------|-----------------|
| Application (GDScript) | Godot engine | Godot export (binary + `.pck`) |
| Extension (Rust) | Native | Compiled `.dylib`/`.dll`/`.so` via GDExtension |
| Camera daemon (Python) | CPython 3.13 | PyInstaller freezes interpreter + all packages |

The user downloads one archive, extracts, and runs. No `pip install`, no `cargo build`, no terminal.

---

## Why PyInstaller

The Python daemon handles camera I/O -- the one task that requires Python bindings (`pco.sdk` for scientific cameras, PyObjC for AVFoundation on macOS). PyInstaller bundles the entire Python runtime, all dependencies (numpy, opencv, pyobjc, pco), and the daemon code into a standalone executable. The user never interacts with Python.

---

## The `.pck` Boundary

Godot's export system packs all `res://` resources into a single `.pck` file. This works for scripts, scenes, shaders, textures -- anything Godot loads at runtime. But:

- The Python daemon is a native executable. It must exist on the real filesystem.
- `OS.create_process()` needs a real filesystem path. It cannot read from `.pck`.
- The GDExtension `.dylib`/`.dll`/`.so` is handled specially by Godot -- it extracts native libraries from the export automatically.

**Rule:** The daemon is never included in Godot's export filters. A post-export packaging script places it alongside the Godot binary.

---

## Daemon Discovery

`PythonUtils` (`src/utils/python_utils.gd`) is the SSoT for finding the daemon:

```
Exported build:
  OS.get_executable_path().get_base_dir() / "openisi-daemon-{arch}" / "openisi-daemon"

Development (editor):
  .venv/bin/python -m daemon.main
```

Detection order:

1. `PythonUtils.is_exported()` -- checks `OS.has_feature("standalone")`
2. If exported: `get_daemon_executable()` -- looks for bundled binary relative to executable
3. If dev: `get_venv_python_path()` -- uses `.venv/bin/python`

On macOS, the universal `.app` contains both `openisi-daemon-arm64/` and `openisi-daemon-x86_64/`. The correct one is selected based on `Engine.get_architecture_name()`.

---

## Daemon Lifecycle in Exported Builds

Same as development, with two differences:

1. **Launch:** Direct executable (`openisi-daemon --width ... --height ...`) instead of `python -m daemon.main ...`
2. **Log location:** `OS.get_user_data_dir() + "/daemon.log"` instead of `{project}/daemon.log` (the `.app` bundle is read-only)

Camera enumeration also uses the bundled executable: `openisi-daemon --enumerate-cameras` outputs JSON to stdout.

---

## Update Checker

`UpdateChecker` autoload queries the GitHub Releases API on startup:

```
GET https://api.github.com/repos/Kim-Neuroscience-Lab/OpenISI/releases/latest
```

Behavior:

- Compares `tag_name` (e.g., `v0.2.0`) against `Version.CURRENT`
- If newer: emits `update_available` signal with version, release notes, download URL, asset size
- If up to date or network unavailable: silent. No error shown. No retry.
- Runs once at startup, after splash screen completes
- Asset matching by platform: `OpenISI-macos.zip`, `OpenISI-windows.zip`, `OpenISI-linux.tar.gz`

---

## In-App Self-Update

When the user clicks "Download & Install":

1. **Download** -- `HTTPRequest` downloads the platform asset to `OS.get_user_data_dir() + "/updates/"`
2. **Extract** -- Unzip/untar to a temporary directory
3. **Write updater script** -- A small platform-specific script:
   - Waits for the current process to exit (polls PID)
   - Replaces the old app with the new one
   - Relaunches the new version
   - Deletes itself
4. **Launch and quit** -- Execute the updater script, then exit the app

This pattern is necessary because a running application cannot replace its own files.

**macOS:**

```bash
#!/bin/bash
# Wait for app to exit
while kill -0 $OLD_PID 2>/dev/null; do sleep 0.5; done
# Replace
rm -rf "$APP_PATH"
mv "$NEW_APP_PATH" "$APP_PATH"
# Relaunch
open "$APP_PATH"
# Cleanup
rm -- "$0"
```

**Windows:** Similar `.bat` script using `taskkill /PID`, `xcopy`, and `start`.

**Linux:** Similar shell script using `kill -0`, `rm -rf`, `mv`.

---

## Version SSoT

`Version` autoload (`src/autoload/version.gd`):

```gdscript
const CURRENT := "0.1.0"
const REPO := "Kim-Neuroscience-Lab/OpenISI"
```

Displayed in the app header. Compared against GitHub Releases tags by the update checker.

Three files track version (kept in sync by `scripts/bump-version.sh`):

| File | Purpose |
|------|---------|
| `src/autoload/version.gd` | App runtime |
| `pyproject.toml` | Python daemon |
| `extension/Cargo.toml` | Rust extension |
