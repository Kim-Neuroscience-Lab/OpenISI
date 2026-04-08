# OpenISI

Open-source intrinsic signal imaging system for retinotopic mapping in mice. Handles camera control, stimulus presentation, data acquisition, and analysis (phase maps, VFS, area segmentation).

Hardware features (PCO camera, stimulus display) require Windows. Analysis and data exploration work on all platforms.

## Setup

Clone and run the setup script. It installs Rust, CMake, the Tauri CLI, and any platform-specific dependencies automatically.

**macOS / Linux:**

```sh
git clone https://github.com/Kim-Neuroscience-Lab/OpenISI.git
cd OpenISI
./scripts/setup.sh
```

**Windows (PowerShell):**

```powershell
git clone https://github.com/Kim-Neuroscience-Lab/OpenISI.git
cd OpenISI
.\scripts\setup.ps1
```

Windows also requires Visual Studio 2022 with the "Desktop development with C++" workload, or the [Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/).

## Running

```sh
cargo tauri dev
```

The first build takes a few minutes (compiling HDF5, wgpu, Tauri). Subsequent launches are fast.

## Quickstart: Analyze sample data

1. **Set a data directory.** In the Library view, click **Set Data Directory** and choose a folder where imported data will be stored.

2. **Download sample data.** Click **Download Sample Data**. This pulls the [SNLC sample dataset](https://github.com/SNLC/ISI) from GitHub, converts each subject's `.mat` files into OpenISI's `.oisi` format, and cleans up the raw download.

3. **Analyze.** Click **Analyze** on any file in the library. Analysis runs automatically on first open — phase maps, VFS, and area segmentation are computed and the VFS map is displayed as an overlay.

4. **Adjust parameters.** The analysis sidebar lets you change smoothing, rotation, angular range, offsets, and segmentation thresholds. Changes re-run the analysis automatically.

## Layer controls

The right sidebar controls the visualization layers, from top to bottom:

- **Base** — the background image. Switch between the live camera feed, the anatomical reference image, or nothing.
- **Map** — the analysis overlay. Select which result map to display (VFS, phase maps, amplitude, eccentricity, etc.), adjust opacity, and set the blend mode.
- **Borders** — toggle the area boundary outlines computed from VFS segmentation.
- **Ring** — toggle the cranial window ring overlay used for spatial calibration.

## Tests

```sh
cargo test --workspace
```

## License

MIT
