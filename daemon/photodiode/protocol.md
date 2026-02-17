# OpenISI Photodiode Protocol

Simple ASCII protocol over USB serial for stimulus timing devices.

## Overview

OpenISI uses photodiodes to capture true hardware timestamps when software-only solutions fail (e.g., macOS Apple Silicon). This protocol allows any compatible device to provide timing data.

## Serial Settings

| Parameter | Value |
|-----------|-------|
| Baud rate | 115200 |
| Data bits | 8 |
| Parity | None |
| Stop bits | 1 |
| Flow control | None |

## Message Format

All messages are ASCII text terminated by newline (`\n`). The first character indicates the message type.

### Device → Host Messages

| Type | Format | Description |
|------|--------|-------------|
| Timestamp | `T<us>\n` | Light detected at timestamp (microseconds since device power-on) |
| Status | `S<code>\n` | Device status: 0=OK, 1=error, 2=threshold changed |
| Info | `I<text>\n` | Device identification string |

**Examples:**
```
T1234567890
S0
I OpenISI-Photodiode v1.0
```

### Host → Device Messages

| Type | Format | Description |
|------|--------|-------------|
| Ping | `P\n` | Request immediate timestamp (for clock synchronization) |
| Threshold | `H<value>\n` | Set detection threshold (0-1023 for 10-bit ADC) |
| Reset | `R\n` | Reset device state |

**Examples:**
```
P
H512
R
```

## Clock Synchronization

The device's `micros()` clock starts at power-on with an arbitrary epoch. The host must establish an offset to convert device timestamps to system time.

### Sync Protocol

1. Host clears serial buffer
2. Host records system time T1
3. Host sends `P\n`
4. Device responds with `T<device_time>\n`
5. Host records system time T2
6. Round-trip time (RTT) = T2 - T1
7. Offset = (T1 + RTT/2) - device_time

Repeat 5 times and take median offset to reject outliers.

### Clock Drift

For long acquisitions (>10 minutes), re-synchronize periodically. Arduino crystal drift is typically <100 ppm.

## Example Session

```
# Device powers on
← I OpenISI-Photodiode v1.0
← S0

# Host syncs clock
→ P
← T1234567

# Host sets threshold
→ H450
← S2

# Acquisition starts, sync patch flashes
← T2345678
← T2362345
← T2379012
← T2395679

# Host requests another timestamp for sync check
→ P
← T3456789

# Acquisition ends
→ R
← S0
```

## Timing Requirements

| Metric | Requirement | Notes |
|--------|-------------|-------|
| Detection latency | <100 µs | Time from photon to serial transmission |
| Jitter | <500 µs | Standard deviation of detection latency |
| Throughput | 60+ events/sec | Match display refresh rate |
| Clock resolution | ≤4 µs | Arduino `micros()` resolution |

## Compatible Devices

Any device implementing this protocol works with OpenISI:

| Device | Detection Latency | Jitter | Cost |
|--------|-------------------|--------|------|
| Arduino Uno | ~50 µs | ~100 µs | $25 |
| Arduino Mega | ~50 µs | ~100 µs | $40 |
| Teensy 4.0 | ~10 µs | ~20 µs | $20 |
| ESP32 | ~30 µs | ~50 µs | $10 |
| Stimulus Onset Hub | ~20 µs | ~20 µs | $200 |

## Reference Implementation

See `hardware/arduino_photodiode/arduino_photodiode.ino` for reference Arduino firmware.

## Extending the Protocol

Manufacturers can add custom messages using lowercase letters (a-z). OpenISI ignores unrecognized message types.

Example vendor extension:
```
# Custom calibration command
→ c
← C auto_threshold=523
```
