"""Correlate photodiode timestamps with Godot software timestamps.

The photodiode captures the TRUE moment when photons hit the screen.
Software timestamps (from frame_post_draw) are captured BEFORE presentation.
This module maps each software timestamp to its corresponding hardware timestamp.

Algorithm:
    For each software timestamp T_software:
        Hardware timestamp = FIRST photodiode timestamp where T_photodiode > T_software

This works because:
    1. frame_post_draw fires after rendering, before vsync wait
    2. Photodiode fires when frame actually appears on screen
    3. Therefore photodiode timestamp is always > software timestamp
"""

import numpy as np
from dataclasses import dataclass
from typing import Optional


@dataclass
class CorrelationResult:
    """Result of correlating software and hardware timestamps."""

    hardware_ts_us: np.ndarray  # Corrected hardware timestamps
    mapping_success: np.ndarray  # Boolean: True if mapping found
    offsets_us: np.ndarray  # Delay from software to hardware (frame latency)

    # Quality metrics
    mean_offset_us: float  # Average frame latency
    std_offset_us: float  # Jitter in frame latency
    max_offset_us: float  # Maximum frame latency
    missing_count: int  # Frames with no matching photodiode event
    hardware_jitter_us: float  # Jitter in hardware timestamps (should be <500us)

    @property
    def success_rate(self) -> float:
        """Fraction of frames successfully mapped."""
        return np.mean(self.mapping_success)

    @property
    def is_valid(self) -> bool:
        """Check if correlation is scientifically valid.

        Requirements:
        - >99% of frames mapped
        - Hardware jitter < 500us (true hardware timing)
        - No unreasonable offsets (< 100ms)
        """
        return (
            self.success_rate > 0.99
            and self.hardware_jitter_us < 500.0
            and self.max_offset_us < 100_000  # 100ms max
        )


def correlate_timestamps(
    software_ts_us: np.ndarray,
    photodiode_ts_us: np.ndarray,
    expected_delta_us: int = 16667,
    max_offset_us: int = 50_000,
) -> CorrelationResult:
    """Map software timestamps to photodiode (hardware) timestamps.

    Args:
        software_ts_us: Software timestamps from Godot frame_post_draw (us)
        photodiode_ts_us: Hardware timestamps from photodiode (us)
        expected_delta_us: Expected frame interval (default 16667 for 60Hz)
        max_offset_us: Maximum allowed offset before marking as missing (50ms)

    Returns:
        CorrelationResult with corrected timestamps and quality metrics.
    """
    n_frames = len(software_ts_us)
    hardware_ts = np.zeros(n_frames, dtype=np.int64)
    mapping_success = np.zeros(n_frames, dtype=bool)
    offsets = []

    if len(photodiode_ts_us) == 0:
        return CorrelationResult(
            hardware_ts_us=hardware_ts,
            mapping_success=mapping_success,
            offsets_us=np.array([]),
            mean_offset_us=0.0,
            std_offset_us=0.0,
            max_offset_us=0.0,
            missing_count=n_frames,
            hardware_jitter_us=0.0,
        )

    # Sort photodiode timestamps (should already be sorted, but ensure it)
    pd_sorted = np.sort(photodiode_ts_us)

    pd_idx = 0
    for i in range(n_frames):
        soft_ts = software_ts_us[i]

        # Find first photodiode timestamp > software timestamp
        while pd_idx < len(pd_sorted) and pd_sorted[pd_idx] <= soft_ts:
            pd_idx += 1

        if pd_idx < len(pd_sorted):
            pd_ts = pd_sorted[pd_idx]
            offset = pd_ts - soft_ts

            # Sanity check: offset should be positive and reasonable
            if 0 < offset < max_offset_us:
                hardware_ts[i] = pd_ts
                mapping_success[i] = True
                offsets.append(offset)
            else:
                # Offset too large - likely missed frame or sync error
                hardware_ts[i] = -1
                mapping_success[i] = False
        else:
            # No more photodiode events
            hardware_ts[i] = -1
            mapping_success[i] = False

    # Calculate quality metrics
    offsets_arr = np.array(offsets) if offsets else np.array([0])

    # Hardware jitter: std dev of intervals between photodiode events
    if len(pd_sorted) > 1:
        pd_deltas = np.diff(pd_sorted)
        # Filter to reasonable deltas (within 2x expected)
        valid_deltas = pd_deltas[(pd_deltas > expected_delta_us * 0.5) &
                                  (pd_deltas < expected_delta_us * 2.0)]
        hardware_jitter = float(np.std(valid_deltas)) if len(valid_deltas) > 1 else 0.0
    else:
        hardware_jitter = 0.0

    return CorrelationResult(
        hardware_ts_us=hardware_ts,
        mapping_success=mapping_success,
        offsets_us=offsets_arr,
        mean_offset_us=float(np.mean(offsets_arr)) if offsets else 0.0,
        std_offset_us=float(np.std(offsets_arr)) if len(offsets) > 1 else 0.0,
        max_offset_us=float(np.max(offsets_arr)) if offsets else 0.0,
        missing_count=int(np.sum(~mapping_success)),
        hardware_jitter_us=hardware_jitter,
    )


def validate_photodiode_timing(
    photodiode_ts_us: np.ndarray,
    expected_hz: float = 60.0,
) -> dict:
    """Validate photodiode timing quality.

    Checks:
    - Detected refresh rate matches expected
    - Jitter is low (<500us for true hardware)
    - No excessive dropped frames

    Args:
        photodiode_ts_us: Photodiode timestamps in microseconds
        expected_hz: Expected refresh rate

    Returns:
        Dictionary with validation results.
    """
    if len(photodiode_ts_us) < 2:
        return {
            "valid": False,
            "error": "Not enough photodiode events",
            "event_count": len(photodiode_ts_us),
        }

    expected_delta_us = 1_000_000.0 / expected_hz

    # Calculate intervals
    deltas = np.diff(photodiode_ts_us)

    # Filter outliers (dropped frames show as 2x+ interval)
    normal_deltas = deltas[deltas < expected_delta_us * 1.5]

    if len(normal_deltas) < 2:
        return {
            "valid": False,
            "error": "Too many dropped frames",
            "dropped_fraction": 1.0 - len(normal_deltas) / len(deltas),
        }

    measured_hz = 1_000_000.0 / np.mean(normal_deltas)
    jitter_us = float(np.std(normal_deltas))
    dropped_count = len(deltas) - len(normal_deltas)

    # Quality checks
    rate_ok = abs(measured_hz - expected_hz) < expected_hz * 0.05  # Within 5%
    jitter_ok = jitter_us < 500.0  # True hardware has <500us jitter
    drops_ok = dropped_count / len(deltas) < 0.01  # <1% drops

    return {
        "valid": rate_ok and jitter_ok and drops_ok,
        "event_count": len(photodiode_ts_us),
        "measured_hz": measured_hz,
        "expected_hz": expected_hz,
        "jitter_us": jitter_us,
        "dropped_count": dropped_count,
        "dropped_fraction": dropped_count / len(deltas),
        "rate_ok": rate_ok,
        "jitter_ok": jitter_ok,
        "drops_ok": drops_ok,
        "is_hardware_timing": jitter_us < 500.0,
    }
