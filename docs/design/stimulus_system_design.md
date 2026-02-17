# Stimulus System Design

## Design Philosophy

**The right level of abstraction**: Fully compositional within paradigms, but paradigms are separate because they represent fundamentally different rendering approaches.

---

## Part 1: Stimulus Paradigms

### Three Rendering Paradigms

| Paradigm | Description | Rendering | Examples |
|----------|-------------|-----------|----------|
| **Texture** | Continuous patterns defined mathematically | Shader-based | Gratings, checkerboards, Gabors, bars, wedges, rings |
| **Element** | Discrete objects with positions | Object-based | Random dots, sparse noise |
| **Media** | External files | Playback | Images, videos |

Each paradigm has its own base class, renderer approach, and parameter structure. They are separate because they cannot be expressed in terms of each other.

---

## Part 2: Texture Paradigm (Compositional)

### Composition Model

```
Texture Stimulus = Carrier × Envelope × Modulation [+ Strobe]
```

| Component | Role | Options |
|-----------|------|---------|
| **Carrier** | The base pattern that fills space | `checkerboard`, `solid` |
| **Envelope** | Spatial windowing / aperture | `bar`, `wedge`, `ring` |
| **Modulation** | Spatial movement of envelope | `static`, `sweep`, `rotate`, `expand` |
| **Strobe** | Temporal contrast reversal (optional) | Checkbox + `strobe_frequency_hz` |

### Retinotopic Stimuli as Compositions

| Stimulus | Carrier | Envelope | Modulation | Strobe |
|----------|---------|----------|------------|--------|
| Drifting bar (solid) | solid | bar | sweep | off |
| Drifting bar (checker) | checkerboard | bar | sweep | on |
| Rotating wedge | checkerboard | wedge | rotate | on |
| Expanding ring | checkerboard | ring | expand | on |
| Static checkerboard | checkerboard | (full-field via renderer) | static | on |

### Parameters by Component

#### Carrier Parameters

| Carrier | Parameters |
|---------|------------|
| `checkerboard` | `check_size_deg` |
| `solid` | (none - uses luminance params only) |

#### Envelope Parameters

| Envelope | Parameters |
|----------|------------|
| `bar` | `stimulus_width_deg` |
| `wedge` | `stimulus_width_deg` (angular width in degrees) |
| `ring` | `stimulus_width_deg`, `max_eccentricity_deg` |

#### Modulation Parameters

| Modulation | Parameters | Directions |
|------------|------------|------------|
| `static` | (none) | (none) |
| `sweep` | `sweep_speed_deg_per_sec` | LR, RL, TB, BT |
| `rotate` | `sweep_speed_deg_per_sec` | CW, CCW |
| `expand` | `sweep_speed_deg_per_sec` | EXP, CON |

#### Strobe Parameters

| Parameter | Description |
|-----------|-------------|
| `strobe_frequency_hz` | Counterphase reversal frequency (only when strobe enabled) |

#### Common Parameters (all texture stimuli)

- `contrast` (0-1)
- `mean_luminance` (0-1)
- `background_luminance` (0-1)

### Direction System

Directions are determined by the **modulation type**:

| Modulation | Directions | Meaning |
|------------|------------|---------|
| `sweep` | LR, RL, TB, BT | Cartesian sweep direction |
| `rotate` | CW, CCW | Rotation direction |
| `expand` | EXP, CON | Expand outward / Contract inward |
| `static` | (none) | No directional component |

---

## Part 3: Element Paradigm

Discrete objects rendered individually. Cannot be expressed as carrier × envelope × modulation.

### Random Dot Kinematogram (RDK)

- `n_dots` - Number of dots
- `dot_size_deg` - Size of each dot
- `coherence` - Fraction moving coherently (0-1)
- `speed_deg_per_sec` - Dot movement speed
- `direction_deg` - Coherent motion direction
- `lifetime_frames` - Dot lifetime before respawn
- `aperture_size_deg` - Circular aperture diameter

### Sparse Noise

- `grid_size` - Number of grid cells (e.g., 16×16)
- `check_size_deg` - Size of each cell
- `n_checks_per_frame` - How many cells flash per frame
- `frame_duration_sec` - Duration of each frame
- `contrast` - Flash contrast

---

## Part 4: Media Paradigm

External files loaded and displayed.

### Image

- `file_path` - Path to image file
- `size_deg` - Display size in degrees
- `position_deg` - Center position

### Video

- `file_path` - Path to video file
- `size_deg` - Display size in degrees
- `position_deg` - Center position
- `loop` - Whether to loop playback

---

## Part 5: Sequence / Presentation Layer

How trials are ordered and presented. This layer is **independent of stimulus paradigm**.

### Sequence Structure Hierarchy

```
SEQUENCE
│
├─ CONDITIONS (ordered list, user-defined via GUI)
│   Example: [TB, BT, LR, RL]
│
├─ REPETITIONS (integer)
│   Example: 10
│
└─ STRUCTURE
    │
    ├─ Interleaved
    │   Cycle through all conditions, repeat n times
    │   │
    │   └─ Cycle Order
    │       ├─ Fixed: TB BT LR RL | TB BT LR RL | ...
    │       └─ Shuffled: (rand) | (rand) | ...
    │
    ├─ Blocked
    │   All repetitions of each condition together
    │   │
    │   └─ Block Order
    │       ├─ Fixed: TB×10 → BT×10 → LR×10 → RL×10
    │       └─ Shuffled: (blocks in random order)
    │
    ├─ Paired
    │   Group conditions into pairs, interleave within pair
    │   │
    │   ├─ Pairs: [(TB, BT), (LR, RL)]
    │   │
    │   └─ Pair Order
    │       ├─ Fixed: (TB BT)×10 → (LR RL)×10
    │       └─ Shuffled: (pairs in random order)
    │
    └─ Fully Randomized
        All trials in completely random order
```

### Sequence Examples

Given conditions `[TB, BT, LR, RL]` with 3 repetitions:

| Structure | Order | Result |
|-----------|-------|--------|
| Interleaved | Fixed | TB BT LR RL \| TB BT LR RL \| TB BT LR RL |
| Interleaved | Shuffled | (RL TB BT LR) \| (BT LR RL TB) \| (LR BT TB RL) |
| Blocked | Fixed | TB TB TB \| BT BT BT \| LR LR LR \| RL RL RL |
| Blocked | Shuffled | LR LR LR \| TB TB TB \| RL RL RL \| BT BT BT |
| Paired | Fixed | TB BT TB BT TB BT \| LR RL LR RL LR RL |
| Paired | Shuffled | LR RL LR RL LR RL \| TB BT TB BT TB BT |
| Fully Random | — | RL TB BT LR TB RL BT TB LR RL LR BT |

### Blank Trials

Optional insertion of blank (gray screen) trials for baseline:

- `enabled` - Whether to insert blanks
- `frequency` - Insert blank every N trials
- `duration_sec` - Duration of blank trial
- `position` - Before or after the Nth trial

---

## Part 6: GUI Design

### Condition Selection (Ordered)

Users build an ordered list by clicking to add:

```
┌─ Conditions ─────────────────────────────────────┐
│                                                   │
│  Available:        Selected (in order):          │
│  ┌─────────┐       ┌─────────────────────┐       │
│  │ [+ LR]  │       │ 1. TB    [↑] [↓] [×]│       │
│  │ [+ RL]  │  →    │ 2. BT    [↑] [↓] [×]│       │
│  │ [+ TB]  │       │ 3. LR    [↑] [↓] [×]│       │
│  │ [+ BT]  │       │ 4. RL    [↑] [↓] [×]│       │
│  └─────────┘       └─────────────────────┘       │
│                                                   │
└───────────────────────────────────────────────────┘
```

### Sequence Structure Selection

```
┌─ Structure ───────────────────────────────────────┐
│                                                   │
│  ○ Interleaved                                   │
│    Cycle through all conditions, repeat          │
│    Order: ○ Fixed  ○ Shuffled each cycle         │
│                                                   │
│  ● Blocked                                       │
│    All repetitions of each condition together    │
│    Order: ● Fixed  ○ Shuffled                    │
│                                                   │
│  ○ Paired                                        │
│    Group into pairs, interleave within pair      │
│    Pairs: [(TB,BT), (LR,RL)]  [Edit pairs...]   │
│    Order: ○ Fixed  ○ Shuffled                    │
│                                                   │
│  ○ Fully Randomized                              │
│    All trials in random order                    │
│                                                   │
└───────────────────────────────────────────────────┘
```

### Sequence Preview

Always show what the user will get:

```
┌─ Preview ─────────────────────────────────────────┐
│  TB TB TB ... (×10) → BT BT BT ... (×10) →       │
│  LR LR LR ... (×10) → RL RL RL ... (×10)         │
│                                                   │
│  Total: 40 sweeps | ~24 min                      │
└───────────────────────────────────────────────────┘
```

---

## Part 7: Data Model (JSON)

### Nameless Protocol Architecture

**The running stimulus has NO identity metadata** — no name, no description, no timestamps. It is simply the current state of configuration values.

- Protocols are only named when **explicitly saved** (to share with others)
- Day-to-day on a single rig: a running, living, nameless protocol
- The filename IS the protocol name when saved

When acquisition starts, the **dataset snapshots** the current Config values. This snapshot becomes part of the immutable scientific record.

### Per-Frame Metadata

Every rendered frame records:

| Field | Type | Description |
|-------|------|-------------|
| `timestamp_us` | int64 | Hardware timestamp in microseconds |
| `condition` | string | Current condition (e.g., "LR", "TB") |
| `sweep_index` | int | Global sweep index (0-indexed) |
| `frame_in_sweep` | int | Frame number within current sweep |
| `progress` | float | 0-1 progress within sweep |
| `state` | string | Sequencer state name |
| `condition_occurrence` | int | Nth time this condition shown (1-indexed) |
| `is_baseline` | bool | True if baseline/inter-trial frame |

### Sequence-Agnostic Analysis

The `condition_occurrence` and `is_baseline` fields enable analysis independent of sequence ordering:

- **Blocked**: First LR sweep has occurrence=1, second LR sweep has occurrence=2
- **Interleaved**: Same — occurrence counts per condition, not globally
- **Randomized**: Same — occurrence tracks repetitions within condition

This allows computing condition-averaged responses without knowing sequence structure.

### Protocol Structure (Saved Snapshots)

```json
{
  "stimulus": {
    "paradigm": "texture",
    "carrier": "checkerboard",
    "envelope": "bar",
    "modulation": "sweep",
    "strobe_enabled": true,
    "params": {
      "check_size_deg": 5.0,
      "stimulus_width_deg": 20.0,
      "sweep_speed_deg_per_sec": 9.0,
      "strobe_frequency_hz": 6.0,
      "contrast": 1.0,
      "mean_luminance": 0.5,
      "background_luminance": 0.5
    }
  },

  "sequence": {
    "conditions": ["TB", "BT", "LR", "RL"],
    "repetitions": 10,
    "structure": {
      "type": "blocked",
      "shuffle": false
    },
    "blanks": {
      "enabled": false,
      "frequency": 5,
      "duration_sec": 10.0
    }
  },

  "timing": {
    "baseline_start_sec": 5.0,
    "baseline_end_sec": 5.0,
    "inter_trial_sec": 2.0
  }
}
```

### Paired Structure Example

```json
"sequence": {
  "conditions": ["TB", "BT", "LR", "RL"],
  "repetitions": 10,
  "structure": {
    "type": "paired",
    "pairs": [["TB", "BT"], ["LR", "RL"]],
    "shuffle": false
  }
}
```

---

## Part 8: Implementation Architecture

### Class Hierarchy

```
StimulusBase (abstract)
│
├── TextureStimulus
│   ├── Composed from: Carrier + Envelope + Modulation
│   ├── Rendered via: Unified shader
│   └── Parameters: Union of component params
│
├── ElementStimulus (abstract)
│   ├── RandomDotStimulus
│   └── SparseNoiseStimulus
│
└── MediaStimulus (abstract)
    ├── ImageStimulus
    └── VideoStimulus
```

### File Structure

```
src/stimulus/
├── stimulus_base.gd
├── stimulus_registry.gd
├── direction_system.gd
├── common_params.gd
│
├── texture/
│   ├── texture_stimulus.gd
│   ├── texture_renderer.gd
│   ├── carriers.gd
│   ├── envelopes.gd
│   ├── modulations.gd
│   └── shaders/
│       └── texture_unified.gdshader
│
├── element/
│   ├── element_stimulus_base.gd
│   ├── random_dot_stimulus.gd
│   └── sparse_noise_stimulus.gd
│
├── media/
│   ├── media_stimulus_base.gd
│   ├── image_stimulus.gd
│   └── video_stimulus.gd
│
├── sequence/
│   ├── sequence_builder.gd
│   └── sequence_structures.gd
│
├── protocol/
│   ├── stimulus_protocol.gd
│   └── timing_config.gd
│
└── display/
    ├── stimulus_display.gd
    └── display_geometry.gd
```

---

## Open Questions

1. **Paired grouping**: Should pairs be auto-detected (opposing directions) or always manually specified?

2. **Multi-factor designs**: Should we support varying multiple parameters (e.g., direction × contrast)?

3. **Orientation stimuli**: For gratings, conditions are orientations rather than directions. Same UI pattern?

4. **Validation**: Which carrier × envelope × modulation combinations should be disallowed?
