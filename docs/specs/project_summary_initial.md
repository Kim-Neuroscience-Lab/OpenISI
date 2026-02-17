# OpenISI — Project Summary

## What It Is

OpenISI is professional scientific software for **Intrinsic Signal Imaging (ISI)** — a neuroscience technique that maps the functional organization of visual cortex. ISI detects small changes in blood oxygenation and light scattering that occur when neurons become active, allowing researchers to create **retinotopic maps** showing which parts of cortex respond to which parts of the visual field.

The software coordinates three real-time systems:

1. **Stimulus presentation** — Moving bars/gratings displayed on a monitor, VSync-locked for frame-accurate timing
2. **Camera acquisition** — High-speed capture of cortical surface, synchronized to stimulus phase
3. **UI/Control** — Experiment configuration, live preview, status monitoring

---

## Who It's For

**Primary users**: Graduate students, postdocs, and PIs in systems neuroscience labs performing widefield optical imaging of rodent visual cortex.

**User context**:

- Working in darkened rooms (often with anesthetized animals)
- Need reliable, predictable software behavior during time-sensitive experiments
- Range from "just want it to work" to power users who want full parameter control
- Often not programmers — they're neuroscientists

---

## What It Improves Upon

**Current state (what you're replacing):**

- Discrete Python scripts run manually in sequence
- No unified interface — terminal commands, separate windows
- Limited error handling and validation
- No live preview or confidence-building before acquisition
- Configuration via code edits or command-line args

**Legacy landscape:**

- Old MATLAB codebases (common in neuroscience, but MATLAB licenses are expensive and the code is often poorly maintained)
- Commercial solutions (expensive, closed, inflexible)
- Lab-specific hacks passed down through grad students

**OpenISI improvements:**

- Unified application with guided workflow
- Phase-aware UI (Setup → Focus → Confirm → Run → Done)
- Maximum auto-detection with computed defaults
- Pre-flight validation and testing before committing to acquisition
- Dark room-friendly interface
- Open source, accessible to all labs

---

## Technical Architecture Decisions

### Why Godot (not Python GUI frameworks)?

| Consideration | Python GUI (PyQt, Tkinter, etc.) | Godot |
|---------------|----------------------------------|-------|
| **Stimulus rendering** | Requires separate window/library (PsychoPy, Pygame) | Native, VSync-locked, same engine |
| **UI quality** | Functional but dated aesthetics | Modern, customizable, polished |
| **Multi-window** | Painful cross-platform | Native support in 4.x |
| **Real-time performance** | GIL issues, inconsistent timing | Game engine — built for real-time |
| **Development experience** | Fragmented ecosystem | Unified scene/node system |
| **Your experience** | Described Python UI frameworks as "so bad" | 2 years experience, comfortable |

### Why Python subprocess for camera?

- **pco.panda SDK** has Python bindings (pco.sdk), no Godot/C++ equivalent
- Camera control is I/O-bound, not UI-bound — doesn't need to be in main process
- Clean separation of concerns: Godot = presentation, Python = acquisition
- Easier to test/develop camera code independently
- If camera crashes, UI survives

### Communication: TCP over localhost

```
┌─────────────────┐         TCP/JSON          ┌─────────────────┐
│     GODOT       │◄────────────────────────►│     PYTHON      │
│  (UI + Stim)    │    localhost:9876         │  (Camera Daemon)│
│                 │                           │                 │
│  - Config UI    │  ──► start_acquisition    │  - pco.sdk      │
│  - Stimulus     │  ──► stop_acquisition     │  - Frame buffer │
│  - Preview      │  ◄── frame_captured       │  - File I/O     │
│  - Status       │  ◄── status_update        │                 │
└─────────────────┘                           └─────────────────┘
```

Why TCP over alternatives:

- **Shared memory**: Faster but complex, platform-specific
- **Pipes**: Simpler but less flexible for bidirectional async
- **Files**: Too slow for real-time status
- **TCP**: Battle-tested, debuggable (can `nc` into it), good enough for status updates at ~30Hz

---

## Key Design Principles

1. **Detect → Compute → Display → Override**
   - Auto-detect everything possible (monitor size, resolution, refresh rate, camera capabilities, GPU)
   - Compute sensible defaults from detected values
   - Display what was detected and computed
   - Allow full override for power users

2. **Confidence over speed**
   - Users should feel certain the system is configured correctly before acquisition
   - Preview and test capabilities at every stage
   - Clear validation with specific error messages

3. **Phase-aware progression**
   - UI adapts to current workflow phase
   - Can't skip critical steps
   - Clear indication of what's happening and what's next

4. **Dark room friendly**
   - Low-brightness color palette
   - High contrast for critical elements
   - Large touch targets
   - No sudden bright flashes

5. **Graceful degradation**
   - Camera disconnect doesn't crash UI
   - Clear error states with recovery paths
   - Always possible to abort safely

---

## Core Workflow Phases

```
┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐
│  SETUP  │───►│  FOCUS  │───►│ CONFIRM │───►│   RUN   │───►│  DONE   │
└─────────┘    └─────────┘    └─────────┘    └─────────┘    └─────────┘
     │              │              │              │              │
  Configure     Live camera    Review all     Acquisition    Results &
  experiment    feed for       settings,      in progress    next steps
  parameters    positioning    run tests
```

---

## File Outputs

Each session produces:

- **Raw frames**: TIFF stack or HDF5 (configurable)
- **Timestamps**: CSV with frame indices and microsecond timestamps (from camera hardware)
- **Metadata**: JSON with full experiment configuration
- **Stimulus log**: Frame-accurate record of what was displayed when

---

## Target Hardware

- **Camera**: pco.panda (primary), with architecture allowing future camera backends
- **Stimulus display**: Secondary monitor, any resolution (auto-detected)
- **Control display**: Primary monitor for UI
- **OS**: Windows (primary, due to pco.sdk), Linux (secondary goal)
- **No external DAQ required**: Software synchronization is sufficient for ISI timescales
