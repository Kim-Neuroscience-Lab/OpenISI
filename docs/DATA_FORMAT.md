# Data Format Specification

## Principles

1. **Hardware timestamps only.** Every timestamp in the `.oisi` file comes from a hardware source — camera internal clock, GPU vsync hardware timing, or QPC (hardware performance counter). No `SystemTime::now()`, no `Instant::now()`, no wall clock readings.

2. **Save raw data.** Camera frames are stored as raw sensor values (u16). No conversion, no normalization, no dF/F. Processing happens in analysis, not export.

3. **Save everything.** Baseline frames, inter-trial frames, all frames — not just stimulus periods. The analysis pipeline decides what to use. The acquisition pipeline saves what the hardware produced.

4. **Per-frame alignment.** Every camera frame has a corresponding stimulus state record. The exact stimulus position at each camera frame's capture time is recorded, not inferred post-hoc.

5. **Clock synchronization.** Camera hardware clock and system QPC clock are explicitly synchronized. The offset is recorded so analysis can align camera timestamps with stimulus timestamps.

6. **Complete provenance.** The file contains everything needed to fully reproduce the analysis — rig geometry, experiment definition, hardware state, timing quality metrics. No external files needed.

## Timestamp sources

### Camera timestamps
**Source:** PCO camera's internal clock, embedded as BCD in the first 14 pixels of each frame.
**Type:** True hardware timestamp from the camera sensor.
**Format:** Microseconds since midnight (decoded from year/month/day/hour/minute/second/microsecond).
**Precision:** Microsecond.

### System timestamps (QPC)
**Source:** Windows QueryPerformanceCounter — a hardware counter on the CPU.
**Type:** Hardware performance counter read in software.
**Format:** Microseconds since arbitrary epoch (QPC / frequency * 1e6).
**Precision:** ~100 nanoseconds.
**Use:** Cross-clock alignment with camera timestamps, and stimulus vsync timing.

### Stimulus vsync timestamps
**Source:** DXGI frame statistics — `IDXGISwapChain::GetFrameStatistics()` returns `SyncQPCTime`, the QPC value at the actual hardware vsync when the frame was presented to the display. Queried after each `Present()` call.
**Type:** True GPU/display hardware vsync timestamp, expressed in QPC units.
**Format:** Microseconds (converted from QPC ticks using QPC frequency).
**Precision:** Sub-microsecond (hardware vsync precision).

**Why not QPC-after-WaitForVBlank:** WaitForVBlank wakes the CPU thread after the vsync occurs, introducing OS scheduling jitter (tens to hundreds of microseconds). DXGI frame statistics report the actual QPC value at the vsync interrupt, bypassing this jitter.

**Why not VK_GOOGLE_display_timing:** This Vulkan extension is not reliably available on Windows. DXGI frame statistics are universally available on all Windows DXGI swap chains and report the same hardware vsync event.

### Clock synchronization
At acquisition start, both clocks are read at the same moment:
- `camera_clock_us`: Camera hardware timestamp of the first frame
- `system_clock_us`: QPC timestamp at the moment that first frame is read

This pair establishes the offset: `offset = system_clock_us - camera_clock_us`. Analysis uses this to convert between clocks.

A second sync point is recorded at acquisition end. The difference between start and end offsets reveals clock drift over the acquisition duration. Analysis can linearly interpolate the offset for intermediate frames.

## Data integrity

All HDF5 datasets use the Fletcher32 checksum filter. This detects silent data corruption from disk errors, memory errors, or incomplete writes. If a checksum fails on read, the data is known to be corrupt rather than silently wrong.

## HDF5 file structure

```
/                                       Root
├── version                             attr: "2.0"
├── software_version                    attr: string (e.g. "0.1.0-alpha")
├── created_at                          attr: ISO-8601 string (for human reference only, not for timing)
│
├── /rig                                group — snapshot of rig config at acquisition time
│   ├── /geometry
│   │   ├── viewing_distance_cm         attr: f64
│   │   ├── horizontal_offset_deg       attr: f64
│   │   ├── vertical_offset_deg         attr: f64
│   │   └── projection                  attr: string ("cartesian", "spherical", "cylindrical")
│   ├── /display
│   │   ├── monitor_name                attr: string
│   │   ├── width_px                    attr: u32
│   │   ├── height_px                   attr: u32
│   │   ├── width_cm                    attr: f64
│   │   ├── height_cm                   attr: f64
│   │   ├── physical_size_source        attr: string ("edid_detailed_timing", "edid_basic", "user_override")
│   │   ├── refresh_hz_reported         attr: f64
│   │   ├── refresh_hz_measured         attr: f64
│   │   ├── measurement_jitter_us       attr: f64
│   │   ├── measurement_sample_count    attr: u32
│   │   ├── rotation_deg                attr: f64
│   │   ├── target_stimulus_fps         attr: u32
│   │   └── gamma_corrected             attr: bool (whether gamma correction was applied to stimulus)
│   └── /camera
│       ├── model                       attr: string
│       ├── serial_number               attr: u32
│       ├── width_px                    attr: u32
│       ├── height_px                   attr: u32
│       ├── exposure_us                 attr: u32
│       ├── gain                        attr: i32
│       └── pixel_rate_hz               attr: u32
│
├── /experiment                         group — snapshot of experiment definition
│   ├── /stimulus
│   │   ├── envelope                    attr: string ("bar", "wedge", "ring", "fullfield")
│   │   ├── carrier                     attr: string ("solid", "checkerboard")
│   │   └── /params
│   │       ├── contrast                attr: f64
│   │       ├── mean_luminance          attr: f64
│   │       ├── background_luminance    attr: f64
│   │       ├── check_size_deg          attr: f64
│   │       ├── check_size_cm           attr: f64
│   │       ├── strobe_frequency_hz     attr: f64
│   │       ├── stimulus_width_deg      attr: f64
│   │       ├── sweep_speed_deg_per_sec attr: f64
│   │       ├── rotation_speed_deg_per_sec attr: f64
│   │       ├── expansion_speed_deg_per_sec attr: f64
│   │       └── rotation_deg            attr: f64
│   ├── /presentation
│   │   ├── conditions                  attr: [string]
│   │   ├── repetitions                 attr: u32
│   │   ├── structure                   attr: string ("blocked", "interleaved")
│   │   └── order                       attr: string ("sequential", "interleaved", "randomized")
│   └── /timing
│       ├── baseline_start_sec          attr: f64
│       ├── baseline_end_sec            attr: f64
│       ├── inter_stimulus_sec          attr: f64
│       └── inter_direction_sec         attr: f64
│
├── /session                            group — session metadata
│   ├── animal_id                       attr: string
│   └── notes                           attr: string
│
├── /anatomical                         dataset: u16 (H, W) — optional reference image
│
├── /acquisition                        group — all acquired data
│   ├── /clock_sync                     group — clock alignment
│   │   ├── start_camera_clock_us       attr: i64 (camera HW timestamp of first frame)
│   │   ├── start_system_clock_us       attr: i64 (QPC at first frame read)
│   │   ├── end_camera_clock_us         attr: i64 (camera HW timestamp of last frame)
│   │   ├── end_system_clock_us         attr: i64 (QPC at last frame read)
│   │   └── qpc_frequency              attr: i64 (QPC ticks per second)
│   │
│   ├── /camera                         group — camera frame data
│   │   ├── frames                      dataset: u16 (T, H, W) chunked + gzip
│   │   │                               All frames in acquisition order (including baselines).
│   │   │                               Raw sensor values. No conversion.
│   │   ├── hardware_timestamps_us      dataset: i64 (T,)
│   │   │                               Camera internal clock, microseconds since midnight.
│   │   ├── system_timestamps_us        dataset: i64 (T,)
│   │   │                               QPC at frame read time. Same clock as stimulus timestamps.
│   │   └── sequence_numbers            dataset: u64 (T,)
│   │                                   Camera frame counter. Gaps indicate hardware-level drops.
│   │
│   ├── /stimulus                       group — stimulus frame data
│   │   ├── vsync_timestamps_us         dataset: i64 (N,)
│   │   │                               Hardware vsync QPC time from DXGI frame statistics.
│   │   │                               This is the actual QPC at the vsync interrupt, not
│   │   │                               a software reading after the fact.
│   │   ├── present_count               dataset: u32 (N,)
│   │   │                               DXGI PresentCount from frame statistics. Gaps indicate
│   │   │                               the GPU dropped a presentation frame.
│   │   ├── state                       dataset: u8 (N,)
│   │   │                               0=idle, 1=baseline_start, 2=sweep, 3=inter_stimulus,
│   │   │                               4=inter_direction, 5=baseline_end, 6=complete
│   │   ├── condition_index             dataset: u8 (N,)
│   │   │                               Index into /experiment/presentation/conditions.
│   │   ├── sweep_index                 dataset: u32 (N,)
│   │   │                               Global sweep counter (0-based across all conditions).
│   │   ├── condition_occurrence        dataset: u32 (N,)
│   │   │                               Which repetition of this condition (0-based).
│   │   └── progress                    dataset: f32 (N,)
│   │                                   0–1 progress within current sweep.
│   │
│   ├── /schedule                       group — realized sweep schedule
│   │   ├── sweep_sequence              attr: [string] — ordered list of conditions as run
│   │   ├── sweep_start_us              dataset: i64 (S,) — QPC at each sweep start
│   │   └── sweep_end_us                dataset: i64 (S,) — QPC at each sweep end
│   │
│   └── /quality                        group — timing quality metrics
│       ├── camera_frame_deltas_us      dataset: i64 (T-1,) — delta between consecutive camera frames
│       ├── camera_dropped_indices      dataset: u32 (D,) — indices where delta > threshold
│       ├── camera_sequence_gaps        dataset: u32 (G,) — indices where sequence number is non-consecutive
│       ├── stimulus_frame_deltas_us    dataset: i64 (N-1,) — delta between consecutive vsync timestamps
│       ├── stimulus_dropped_indices    dataset: u32 (E,) — indices where present_count is non-consecutive
│       ├── mean_frame_intensity        dataset: f32 (T,) — mean pixel value per camera frame
│       │                               Reveals illumination drift, photobleaching, or tissue movement.
│       ├── expected_camera_delta_us    attr: i64 — expected frame period from target FPS
│       ├── expected_stimulus_delta_us  attr: i64 — expected vsync period from display refresh
│       ├── camera_drops_total          attr: u32 — total camera frames flagged as dropped
│       ├── stimulus_drops_total        attr: u32 — total stimulus frames flagged as dropped
│       └── acquisition_complete        attr: bool — false if acquisition was aborted or interrupted
│
├── /complex_maps                       group — computed by analysis
│   ├── azi_fwd                         dataset: f64 (H, W, 2) — real + imaginary
│   ├── azi_rev                         dataset: f64 (H, W, 2)
│   ├── alt_fwd                         dataset: f64 (H, W, 2)
│   └── alt_rev                         dataset: f64 (H, W, 2)
│
├── /results                            group — computed by analysis
│   ├── azi_phase                       dataset: f64 (H, W)
│   ├── alt_phase                       dataset: f64 (H, W)
│   ├── azi_phase_degrees               dataset: f64 (H, W)
│   ├── alt_phase_degrees               dataset: f64 (H, W)
│   ├── azi_amplitude                   dataset: f64 (H, W)
│   ├── alt_amplitude                   dataset: f64 (H, W)
│   └── vfs                             dataset: f64 (H, W)
│
└── /analysis_params                    attr: JSON string (AnalysisParams used to produce results)
```

## Key design decisions

### All frames saved in acquisition order
Camera frames are stored as a single contiguous array `(T, H, W)` in the order they were captured, not grouped by condition. This preserves:
- Baseline frames (needed for dF/F denominator)
- Inter-trial frames (needed for signal decay analysis)
- The actual temporal structure of the acquisition

The `/acquisition/camera_stimulus_alignment` group provides per-camera-frame stimulus state, so analysis can group frames by condition, repetition, or state without losing the temporal context.

### Raw u16 pixels, not float
Camera sensor values are stored as the original u16 values from the sensor. No normalization, no dF/F, no type conversion. This:
- Preserves the full dynamic range of the sensor
- Halves file size compared to f32
- Lets analysis choose the processing pipeline (dF/F, dR/R, trial-subtraction, etc.)

### Stimulus state is computed, not stored per camera frame
The stimulus state at any camera frame's capture time is a deterministic function of the experiment definition (timing parameters, sweep schedule) and the frame's timestamp. Analysis computes it analytically — no runtime alignment data is needed. The sweep schedule (`/acquisition/schedule/`) provides the realized timing, and the camera timestamps provide when each frame was captured. The lookup is exact, not interpolated.

### Clock synchronization and drift detection
The `/acquisition/clock_sync` group records the relationship between the camera's internal clock and the system QPC clock at both acquisition start and end. This enables:
- Analysis code to convert between clock domains
- Drift detection: if the offset differs between start and end, the clocks drifted. Analysis can linearly interpolate the offset for intermediate frames.
- Cross-validation of frame timing

### Sequence number gap detection
Camera sequence numbers are saved so analysis can detect hardware-level frame drops that timing thresholds might miss. If sequence numbers 45, 46, 48 appear, frame 47 was dropped by the camera — regardless of whether the timestamp delta exceeded the detection threshold.

## Storage estimates

For a typical acquisition (4 conditions × 10 reps, 8s sweeps + 5s baselines, 30 fps, 960×600 after 2×2 binning):
- Total duration: ~12 minutes = 720 seconds
- Total frames: 720 × 30 = 21,600
- Frame size: 960 × 600 × 2 bytes = 1.15 MB
- Raw frames: 21,600 × 1.15 MB = ~24 GB uncompressed
- With gzip (level 4): ~8–12 GB (intrinsic signal changes are small, compression is effective)
- Timestamp arrays: negligible (<1 MB)

Without binning (1920×1200):
- Raw frames: ~96 GB uncompressed, ~30–40 GB compressed

Labs should plan storage accordingly. Binning is recommended for standard retinotopy.

## What analysis needs from this format

### Standard Fourier retinotopy (Kalatsky & Stryker 2003)
1. Read `/acquisition/camera/frames` — all frames
2. Read `/acquisition/camera_stimulus_alignment/condition_index` — group frames by condition
3. Read `/acquisition/camera_stimulus_alignment/stimulus_state` — separate sweep frames from baseline
4. Compute dF/F using baseline frames as denominator
5. Average across repetitions of same condition
6. DFT at stimulus frequency (period = sweep duration from timestamps)
7. Phase extraction → retinotopic map

### Delay-corrected mapping (Marshel et al. 2011)
Same as above, plus:
1. Forward and reverse maps combined: Z = fwd × conj(rev)
2. Phase = angle(Z) / 2 → delay-corrected retinotopic position

### Trial-by-trial analysis
1. Use `/acquisition/camera_stimulus_alignment/condition_occurrence` to separate repetitions
2. Compute per-trial maps
3. Statistical testing across trials

### Timing quality assessment
1. Read `/acquisition/camera/hardware_timestamps_us` — compute frame deltas
2. Read `/acquisition/camera/sequence_numbers` — check for gaps
3. Read `/acquisition/stimulus/vsync_timestamps_us` — compute vsync deltas
4. Compare expected vs actual frame rates
5. Flag periods of poor timing for exclusion

### Visual field sign map
1. Requires both azimuth and altitude acquisitions (separate .oisi files)
2. Load phase maps from each
3. Compute spatial gradients
4. VFS = sin(angle between gradient vectors)

## Scientific integrity

### Stimulus frequency in analysis
The DFT projection must use the **defined** stimulus frequency (1 / sweep_duration, computed from experiment config + rig geometry), NOT a frequency measured from timestamps. Measured frequency is affected by clock drift and dropped frames. The experiment definition is the ground truth for what the stimulus was.

### Gamma correction
Monitors have nonlinear luminance response (typically gamma ≈ 2.2). A pixel value of 128 does not produce 50% luminance. The `/rig/display/gamma_corrected` flag records whether the stimulus renderer applied gamma correction. Most published ISI studies do NOT gamma-correct, and phase maps are unaffected (only amplitude is affected). But the flag ensures reproducibility — two labs can compare results knowing whether correction was applied.

### Mean frame intensity
The `/acquisition/quality/mean_frame_intensity` array records the mean pixel value of every camera frame. This is a low-cost quality metric that reveals:
- **Illumination drift** — gradual change in light source intensity
- **Photobleaching** — progressive darkening of tissue
- **Tissue movement** — sudden shifts from brain pulsation or animal movement
- **Vignetting changes** — if the objective shifts

A stable mean intensity is a necessary (not sufficient) condition for clean dF/F.

### Software version
The `/software_version` attribute records which version of OpenISI produced the file. If a bug is discovered in the renderer or analysis pipeline, affected datasets can be identified and reprocessed.

### Plausibility warnings
The system warns (but does not prevent) implausible configurations:
- Viewing distance < 1 cm or > 200 cm
- Monitor physical dimensions that imply DPI outside 20–600 range
- Camera exposure > 90% of frame period
- Baseline duration < 1 second
- Stimulus speed that produces sweep duration < 2 seconds or > 60 seconds

These are warnings in the UI, not hard limits. The scientist may have valid reasons for unusual values.

### Checksums
All HDF5 datasets use the Fletcher32 checksum filter. On read, checksum failure indicates data corruption. This catches silent disk errors, incomplete writes, and file transfer corruption.

### Acquisition completeness
The `/acquisition/quality/acquisition_complete` flag is `false` if the acquisition was aborted, interrupted by hardware failure, or stopped early. Analysis code should check this flag and handle incomplete data explicitly rather than silently processing a truncated dataset.
