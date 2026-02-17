# OpenISI Photodiode Timestamp Capture

Hardware timestamps for display timing validation using a photodiode attached to the screen.

## Why This Exists

On macOS Apple Silicon, there's no software-only way to get true hardware vsync timestamps:
- `VK_GOOGLE_display_timing` returns garbage data
- `MTLDrawable.presentedTime` is iOS-only
- `CVDisplayLink` is just a timer, not actual vsync

A photodiode measuring actual photon emission is the scientific standard for display timing validation.

## Hardware Required

| Component | Description | Cost |
|-----------|-------------|------|
| Arduino Uno/Mega | Microcontroller | ~$25 |
| BPW34 | Silicon photodiode | ~$2 |
| 390kΩ resistor | Pull-up resistor | ~$0.10 |
| Suction cup | Mount to screen | ~$5 |

**Total: ~$35**

## Circuit

```
    VCC (5V)
      │
      ├──[390kΩ]──┬── Analog Pin A0
      │           │
      │       [BPW34]
      │       (photodiode)
      │           │
      └───────────┴── GND
```

The photodiode is reverse-biased. When light hits it, current flows and the voltage at A0 drops.

## Assembly

1. Connect 390kΩ resistor between 5V and A0
2. Connect BPW34 cathode (shorter leg, or marked) to A0
3. Connect BPW34 anode to GND
4. Mount photodiode in suction cup holder pointing at screen

## Installation

1. Open `arduino_photodiode.ino` in Arduino IDE
2. Select your board (Arduino Uno or Mega)
3. Upload to board
4. Open Serial Monitor at 115200 baud to verify

## Usage

1. Position photodiode over the sync patch on your display
2. Start OpenISI acquisition
3. The sync patch will flash white/black each frame
4. Photodiode detects the flash and sends timestamp

## Calibration

The default threshold (512) works for most setups. If you're getting:
- **Too many events**: Increase threshold (send `H600\n`)
- **Missing events**: Decrease threshold (send `H400\n`)

## Protocol

See `daemon/photodiode/protocol.md` for the serial protocol specification.

## Troubleshooting

**No events detected:**
- Check wiring (cathode vs anode)
- Increase photodiode sensitivity by decreasing resistor (try 220kΩ)
- Decrease threshold

**Too many events:**
- Increase threshold
- Shield photodiode from ambient light
- Use smaller sync patch

**Jitter > 500µs:**
- This is expected for Arduino (4µs timer resolution)
- For better precision, use Teensy or ESP32

## References

- [Stimulus Onset Hub](https://stimulusonsethub.github.io/StimulusOnsetHub/) - Open-source design this is based on
- [Black Box ToolKit](https://www.blackboxtoolkit.com/) - Commercial alternative
