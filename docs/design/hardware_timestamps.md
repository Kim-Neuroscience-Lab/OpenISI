# Hardware Timestamps

## Overview

OpenISI requires **true hardware timestamps** for scientific validity. Software timestamps are unacceptable because they don't reflect when stimuli actually appeared on screen or when camera frames were actually captured.

This document describes the hardware timestamp system for both stimulus display and camera acquisition.

---

## The Problem

### Why Software Timestamps Fail

Software timestamps measure when code *thinks* something happened, not when it *actually* happened:

| Event | Software Timestamp | Actual Event |
|-------|-------------------|--------------|
| Frame rendered | `frame_post_draw` fires | Could be 5-20ms before display |
| Vsync | OS reports "done" | Compositor may add variable delay |
| Camera capture | Driver callback | May be buffered, delayed |

For scientific data, we need to know the **true physical moment**:
- When photons hit the retina (stimulus timing)
- When photons hit the sensor (camera timing)

### Platform-Specific Issues

| Platform | VK_GOOGLE_display_timing | MTLDrawable.presentedTime | Notes |
|----------|--------------------------|---------------------------|-------|
| **macOS Apple Silicon** | Returns garbage (3990µs jitter) | API_UNAVAILABLE | No software solution exists |
| **macOS Intel** | May work via MoltenVK | API_UNAVAILABLE | Unreliable |
| **Linux** | Works with native Vulkan | N/A | Preferred platform |
| **Windows** | Works with native Vulkan | N/A | Works well |

From [Psychtoolbox documentation](https://psychtoolbox.org/docs/SyncTrouble):
> "For macOS running on ARM based Macs with 'Apple silicon'... there is currently no known way to prevent the desktop compositor from interfering and therefore visual stimulation timing must be considered unfixably broken."

---

## Solution: Multi-Layer Approach

OpenISI uses multiple timestamp sources with automatic fallback:

```
Priority 1: Photodiode (TRUE hardware, all platforms)
     ↓
Priority 2: VK_GOOGLE_display_timing (Linux/Windows)
     ↓
Priority 3: FAIL - refuse to record invalid data
```

**There is no software fallback.** If hardware timestamps are unavailable, acquisition refuses to proceed.

---

## Photodiode Hardware Timestamps

### How It Works

1. **Sync Patch**: A 50×50 pixel white/black square toggles each frame in the corner of the display
2. **Photodiode**: Attached to the screen, detects light changes with µs precision
3. **Microcontroller**: Arduino captures timestamp when light crosses threshold
4. **Correlation**: Daemon maps photodiode timestamps to software frame indices

```
┌─────────────────────────────────────────────────────────┐
│                    Display                               │
│  ┌──────┐                                               │
│  │ Sync │  ←── Photodiode attached here                 │
│  │Patch │                                               │
│  └──────┘                                               │
│                                                         │
│              [ Stimulus Content ]                       │
│                                                         │
└─────────────────────────────────────────────────────────┘
         │
         │ Light pulse
         ▼
    ┌──────────┐
    │Photodiode│ ──→ Arduino ──→ USB Serial ──→ Daemon
    └──────────┘      (T1234567)
```

### Timing Accuracy

| Component | Latency | Jitter |
|-----------|---------|--------|
| Photodiode (BPW34) | 15 µs | <5 µs |
| Arduino ADC + processing | 50 µs | <10 µs |
| USB Serial transmission | Variable | <1 ms |
| **Total detection** | ~70 µs | <100 µs |

This is **orders of magnitude better** than software timing (3990 µs jitter on macOS).

### Hardware Options

| Option | Cost | Jitter | Notes |
|--------|------|--------|-------|
| DIY Arduino + BPW34 | ~$50 | <100 µs | Recommended for most users |
| [Stimulus Onset Hub](https://stimulusonsethub.github.io/StimulusOnsetHub/) | ~$200 | <20 µs | Open-source, peer-reviewed |
| [Black Box ToolKit](https://www.blackboxtoolkit.com/) | ~$2000 | <1 ms | Commercial, turnkey |

### Bill of Materials (DIY ~$50)

| Component | Description | Source | Cost |
|-----------|-------------|--------|------|
| Arduino Uno | Microcontroller | Amazon/Adafruit | $25 |
| BPW34 | Silicon photodiode | Mouser/DigiKey | $2 |
| 390kΩ resistor | Pull-up | Any | $0.10 |
| USB cable | Type A to B | Any | $5 |
| Suction cup | Mount to screen | Amazon | $5 |
| Enclosure | Optional, 3D printed | - | $10 |

### Circuit

```
    VCC (5V)
      │
      ├──[390kΩ]──┬── Analog Pin A0
      │           │
      │       [BPW34]
      │       (cathode to junction)
      │           │
      └───────────┴── GND
```

The photodiode is reverse-biased. Light increases current flow, dropping the voltage at A0.

### Assembly

1. Connect 390kΩ resistor between Arduino 5V and A0
2. Connect BPW34 cathode (shorter leg/marked) to A0 junction
3. Connect BPW34 anode to GND
4. Mount in suction cup holder, facing the screen
5. Position over sync patch (top-left corner of stimulus display)

### Firmware

Flash `hardware/arduino_photodiode/arduino_photodiode.ino` to the Arduino.

Test with Serial Monitor at 115200 baud:
```
I OpenISI-Photodiode v1.0
S0
```

Wave your hand over the photodiode - you should see `T<timestamp>` messages.

---

## OpenISI Photodiode Protocol

Simple ASCII protocol over USB serial. Any device implementing this protocol works with OpenISI.

### Serial Settings

| Parameter | Value |
|-----------|-------|
| Baud rate | 115200 |
| Data bits | 8 |
| Parity | None |
| Stop bits | 1 |

### Messages

#### Device → Host

| Message | Format | Description |
|---------|--------|-------------|
| Timestamp | `T<us>\n` | Light detected, timestamp in microseconds |
| Status | `S<code>\n` | 0=OK, 1=error |
| Info | `I<text>\n` | Device identification |

#### Host → Device

| Message | Format | Description |
|---------|--------|-------------|
| Ping | `P\n` | Request timestamp (for clock sync) |
| Threshold | `H<val>\n` | Set detection threshold (0-1023) |
| Reset | `R\n` | Reset device state |

### Clock Synchronization

The device clock starts at power-on (arbitrary epoch). The daemon synchronizes it with system time using ping-pong RTT measurement:

1. Record system time T1
2. Send `P\n`
3. Device responds `T<device_time>\n`
4. Record system time T2
5. Offset = (T1 + (T2-T1)/2) - device_time

See `daemon/photodiode/protocol.md` for full specification.

---

## Camera Hardware Timestamps

Cameras provide hardware timestamps through their native APIs:

| Camera | Timestamp Source | Accuracy |
|--------|-----------------|----------|
| PCO scientific cameras | Hardware counter | <1 µs |
| AVFoundation (macOS) | CMSampleBuffer presentationTimeStamp | ~1 ms |
| OpenCV (fallback) | Software only | ~10 ms |

The camera daemon (`daemon/camera/`) extracts hardware timestamps and passes them through shared memory to Godot.

---

## Timestamp Correlation

At the end of acquisition, OpenISI correlates all timestamp sources:

```
Software timestamps (Godot frame_post_draw)
         │
         ▼
┌─────────────────────────────────────────────┐
│            Timestamp Correlator              │
│                                              │
│  For each software_ts:                       │
│    hardware_ts = first photodiode_ts        │
│                  where photodiode_ts > soft │
│                                              │
└─────────────────────────────────────────────┘
         │
         ▼
Hardware timestamps (photodiode or VK_GOOGLE_display_timing)
         │
         ▼
Camera timestamps (camera hardware)
         │
         ▼
Final aligned dataset
```

### Quality Metrics

The correlator validates timestamp quality:

| Metric | Requirement | Meaning |
|--------|-------------|---------|
| Hardware jitter | < 500 µs | True hardware timing |
| Mapping success | > 99% | All frames have hardware timestamps |
| Mean offset | Consistent | frame_post_draw → vsync delay |

If quality checks fail, the dataset is marked as **invalid**.

---

## Usage

### Enabling Photodiode Timestamps

1. Connect Arduino with photodiode to USB
2. Position photodiode over sync patch
3. Start OpenISI - photodiode is auto-detected
4. Run acquisition

The sync patch (top-left corner) toggles automatically during recording.

### Verifying Hardware Timestamps

Check the dataset metadata after acquisition:

```json
{
  "recording": {
    "hardware_timestamps": true,
    "timestamp_source": "photodiode",
    "hardware_jitter_us": 87.3,
    "timestamps_finalized": true
  }
}
```

If `hardware_timestamps` is `false`, the data should not be used for scientific analysis.

### Troubleshooting

| Problem | Cause | Solution |
|---------|-------|----------|
| No photodiode events | Wrong threshold | Send `H400\n` to lower threshold |
| Too many events | Ambient light | Shield photodiode, raise threshold |
| High jitter (>500µs) | Not hardware timing | Check VK_GOOGLE_display_timing on Linux |
| Missing frames | Photodiode not aligned | Reposition over sync patch |

---

## Platform Recommendations

| Platform | Recommended Approach |
|----------|---------------------|
| **macOS Apple Silicon** | Photodiode (required) |
| **macOS Intel** | Photodiode (recommended) |
| **Linux** | VK_GOOGLE_display_timing, photodiode optional |
| **Windows** | VK_GOOGLE_display_timing, photodiode optional |

For maximum reliability across all platforms, use a photodiode.

---

## References

- [Stimulus Onset Hub](https://www.frontiersin.org/articles/10.3389/fninf.2020.00002/full) - Open-source design (Frontiers in Neuroinformatics)
- [Black Box ToolKit](https://www.blackboxtoolkit.com/) - Commercial timing validation
- [Psychtoolbox SyncTrouble](https://psychtoolbox.org/docs/SyncTrouble) - macOS timing issues
- [CVDisplayLink article](https://thume.ca/2017/12/09/cvdisplaylink-doesnt-link-to-your-display/) - Why software timing fails
