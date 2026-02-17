# OpenISI

Open-source software for **Intrinsic Signal Imaging (ISI)** — a neuroscience technique that maps the functional organization of visual cortex by detecting small changes in blood oxygenation when neurons become active. OpenISI coordinates VSync-locked stimulus presentation, high-speed camera acquisition, and frame-accurate timing metadata in a single application with a guided workflow. It is built for neuroscientists running retinotopic mapping experiments, not programmers.

---

## Features

- **VSync-locked stimulus presentation** — drifting bars, rotating wedges, expanding rings
- **Multi-camera support** — pco.panda scientific cameras, AVFoundation (macOS), OpenCV fallback
- **Real-time acquisition metrics** — frame rate, dropped frames, timing quality
- **Frame-accurate timing** with hardware timestamp support
- **Automated display refresh rate validation**
- **Cross-platform** — macOS, Windows, Linux
- **Self-contained download** — embedded Python runtime, no dependencies to install
- **In-app update checking**

---

## Requirements

### Hardware

| Component | Research use | Testing / development |
|-----------|-------------|----------------------|
| **Camera** | pco.panda scientific camera | Any USB webcam |
| **Stimulus display** | Second monitor (any resolution, auto-detected) | Second monitor |

### Software

None. OpenISI is a self-contained download — no Python, no drivers (except pco hardware drivers, see [PCO Camera Setup](#pco-camera-setup)), no package managers.

### Operating Systems

| OS | Version | Architecture |
|----|---------|--------------|
| macOS | 12+ | ARM64 (Apple Silicon) and Intel |
| Windows | 10+ | x86_64 |
| Linux | — | x86_64 |

---

## Installation

### macOS

1. Download `OpenISI-macos.zip` from [GitHub Releases](https://github.com/Kim-Neuroscience-Lab/OpenISI/releases)
2. Extract and drag `OpenISI.app` to Applications
3. **First launch:** Right-click the app → **Open** → **Open**. This is required because the app is not yet signed with an Apple Developer certificate. You only need to do this once.

### Windows

1. Download `OpenISI-windows.zip` from [GitHub Releases](https://github.com/Kim-Neuroscience-Lab/OpenISI/releases)
2. Extract to a folder
3. Run `OpenISI.exe`

> Windows SmartScreen may show a warning. Click **More info** → **Run anyway**.

### Linux

1. Download `OpenISI-linux.tar.gz` from [GitHub Releases](https://github.com/Kim-Neuroscience-Lab/OpenISI/releases)
2. Extract and make executable:
   ```
   tar xzf OpenISI-linux.tar.gz
   chmod +x OpenISI
   ```
3. Run `./OpenISI`

---

## Quick Start

1. **Open OpenISI**

2. **Setup** — Select your camera and stimulus display. The app auto-detects available hardware and validates the display refresh rate.

3. **Focus** — View the live camera feed, adjust exposure, and capture an anatomical reference image.

4. **Stimulus** — Design your visual stimulus protocol: pattern, envelope, directions, timing, and repetitions.

5. **Acquire** — Start acquisition. Monitor real-time metrics (frame rate, dropped frames, timing quality) while the stimulus plays and the camera captures.

6. **Results** — Review acquisition quality. Output files are saved to your session directory.

---

## PCO Camera Setup

If you are using a **pco.panda** camera, you need to install [pco.camware](https://www.pco.de) separately. The pco.camware installer provides the USB hardware drivers that the operating system needs to communicate with the camera.

The pco SDK itself is bundled with OpenISI — you do not need to install it. But the hardware drivers are system-level and must be installed through pco's installer.

See [pco.de](https://www.pco.de) for downloads and documentation.

---

## Building from Source

See [docs/BUILDING.md](docs/BUILDING.md) for developer setup instructions.

---

## Contributing

Contributions welcome. See [docs/BUILDING.md](docs/BUILDING.md) for development setup. Open an issue or pull request on [GitHub](https://github.com/Kim-Neuroscience-Lab/OpenISI).

---

## License

MIT License. See [LICENSE](LICENSE) for details.
