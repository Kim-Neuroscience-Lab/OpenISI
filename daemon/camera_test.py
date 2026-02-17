#!/usr/bin/env python3
"""CLI test for camera hardware timestamps and timing performance.

Tests camera capture without requiring Godot. Directly uses camera backends
to measure timing accuracy, jitter, and dropped frames.

Usage:
    poetry run python -m daemon.camera_test [OPTIONS]

Examples:
    # Test default camera for 5 seconds
    poetry run python -m daemon.camera_test

    # Auto-detect camera format (recommended for built-in cameras)
    poetry run python -m daemon.camera_test --auto-format

    # Test specific camera with custom duration
    poetry run python -m daemon.camera_test --camera avfoundation --device 0 --duration 10

    # Test at specific resolution and frame rate
    poetry run python -m daemon.camera_test --width 1280 --height 720 --fps 60

    # List available cameras
    poetry run python -m daemon.camera_test --list
"""

import argparse
import sys
import time
from dataclasses import dataclass
from typing import Optional

import numpy as np

from .config import FrameConfig


@dataclass
class TimingStats:
    """Camera timing statistics."""

    frame_count: int
    elapsed_sec: float
    actual_fps: float
    target_fps: float

    # Interval statistics (microseconds)
    mean_interval_us: float
    std_interval_us: float
    min_interval_us: int
    max_interval_us: int

    # Jitter metrics
    jitter_us: float  # Standard deviation
    jitter_percent: float  # As percentage of expected interval

    # Dropped frames
    expected_frames: int
    dropped_frames: int
    dropped_percent: float

    # Timestamp validation
    timestamps_monotonic: bool
    first_timestamp_us: int
    last_timestamp_us: int

    # Quality assessment
    hardware_timestamps: bool
    is_valid: bool
    failure_reasons: list[str]


def compute_stats(
    timestamps_us: list[int],
    elapsed_sec: float,
    target_fps: float,
    hardware_timestamps: bool,
) -> TimingStats:
    """Compute timing statistics from captured timestamps."""

    frame_count = len(timestamps_us)
    expected_interval_us = 1_000_000.0 / target_fps
    expected_frames = int(elapsed_sec * target_fps)

    # Check for minimum frames
    if frame_count < 2:
        return TimingStats(
            frame_count=frame_count,
            elapsed_sec=elapsed_sec,
            actual_fps=0.0,
            target_fps=target_fps,
            mean_interval_us=0.0,
            std_interval_us=0.0,
            min_interval_us=0,
            max_interval_us=0,
            jitter_us=0.0,
            jitter_percent=0.0,
            expected_frames=expected_frames,
            dropped_frames=expected_frames,
            dropped_percent=100.0,
            timestamps_monotonic=True,
            first_timestamp_us=timestamps_us[0] if timestamps_us else 0,
            last_timestamp_us=timestamps_us[-1] if timestamps_us else 0,
            hardware_timestamps=hardware_timestamps,
            is_valid=False,
            failure_reasons=["Not enough frames captured"],
        )

    # Compute intervals
    intervals = np.diff(timestamps_us)

    # Basic stats - use hardware timestamp span for accurate FPS
    timestamp_span_sec = (timestamps_us[-1] - timestamps_us[0]) / 1_000_000.0
    actual_fps = (frame_count - 1) / timestamp_span_sec if timestamp_span_sec > 0 else 0.0
    mean_interval = float(np.mean(intervals))
    std_interval = float(np.std(intervals))
    min_interval = int(np.min(intervals))
    max_interval = int(np.max(intervals))

    # Jitter
    jitter_us = std_interval
    jitter_percent = (std_interval / expected_interval_us) * 100.0

    # Dropped frames (intervals > 1.5x expected)
    dropped_threshold = expected_interval_us * 1.5
    dropped_intervals = np.sum(intervals > dropped_threshold)
    dropped_percent = (dropped_intervals / len(intervals)) * 100.0

    # Monotonicity check
    timestamps_monotonic = bool(np.all(intervals > 0))

    # Quality assessment
    failure_reasons = []

    # Must have hardware timestamps
    if not hardware_timestamps:
        failure_reasons.append("No hardware timestamps - software timing only")

    # Jitter check: < 10% of frame interval for hardware, < 5% ideal
    if jitter_percent > 10.0:
        failure_reasons.append(f"Jitter too high: {jitter_percent:.1f}% (should be <10%)")

    # FPS check: within 5% of target
    fps_error = abs(actual_fps - target_fps) / target_fps * 100
    if fps_error > 5.0:
        failure_reasons.append(f"FPS error: {fps_error:.1f}% (should be <5%)")

    # Dropped frames: < 1%
    if dropped_percent > 1.0:
        failure_reasons.append(f"Dropped frames: {dropped_percent:.1f}% (should be <1%)")

    # Monotonicity
    if not timestamps_monotonic:
        failure_reasons.append("Timestamps not monotonic")

    is_valid = len(failure_reasons) == 0

    return TimingStats(
        frame_count=frame_count,
        elapsed_sec=elapsed_sec,
        actual_fps=actual_fps,
        target_fps=target_fps,
        mean_interval_us=mean_interval,
        std_interval_us=std_interval,
        min_interval_us=min_interval,
        max_interval_us=max_interval,
        jitter_us=jitter_us,
        jitter_percent=jitter_percent,
        expected_frames=expected_frames,
        dropped_frames=int(dropped_intervals),
        dropped_percent=dropped_percent,
        timestamps_monotonic=timestamps_monotonic,
        first_timestamp_us=timestamps_us[0],
        last_timestamp_us=timestamps_us[-1],
        hardware_timestamps=hardware_timestamps,
        is_valid=is_valid,
        failure_reasons=failure_reasons,
    )


def print_stats(stats: TimingStats) -> None:
    """Print timing statistics in a readable format."""

    print("\n=== Camera Timing Test Results ===\n")

    # Basic info
    print(f"Frames captured: {stats.frame_count}")
    print(f"Duration:        {stats.elapsed_sec:.2f} sec")
    print(f"Target FPS:      {stats.target_fps:.2f}")
    print(f"Actual FPS:      {stats.actual_fps:.2f}")

    # Interval stats
    print(f"\n--- Interval Statistics ---")
    print(f"Mean interval:   {stats.mean_interval_us:.0f} us ({stats.mean_interval_us/1000:.2f} ms)")
    print(f"Std deviation:   {stats.std_interval_us:.0f} us (jitter)")
    print(f"Min interval:    {stats.min_interval_us} us")
    print(f"Max interval:    {stats.max_interval_us} us")

    # Jitter
    print(f"\n--- Jitter Analysis ---")
    print(f"Jitter:          {stats.jitter_us:.0f} us ({stats.jitter_percent:.2f}% of frame interval)")

    # Dropped frames
    print(f"\n--- Frame Drops ---")
    print(f"Expected frames: {stats.expected_frames}")
    print(f"Dropped frames:  {stats.dropped_frames} ({stats.dropped_percent:.2f}%)")

    # Timestamp info
    print(f"\n--- Timestamp Validation ---")
    print(f"Hardware timestamps: {'YES' if stats.hardware_timestamps else 'NO'}")
    print(f"Monotonic:       {'YES' if stats.timestamps_monotonic else 'NO'}")
    print(f"First timestamp: {stats.first_timestamp_us} us")
    print(f"Last timestamp:  {stats.last_timestamp_us} us")
    print(f"Span:            {(stats.last_timestamp_us - stats.first_timestamp_us) / 1_000_000:.2f} sec")

    # Quality assessment
    print(f"\n=== Quality Assessment ===\n")

    expected_interval_us = 1_000_000.0 / stats.target_fps

    # Individual checks
    hw_ok = stats.hardware_timestamps
    jitter_ok = stats.jitter_percent < 10.0
    fps_ok = abs(stats.actual_fps - stats.target_fps) / stats.target_fps < 0.05
    drops_ok = stats.dropped_percent < 1.0
    mono_ok = stats.timestamps_monotonic

    def status(ok: bool) -> str:
        return "[PASS]" if ok else "[FAIL]"

    print(f"  {status(hw_ok)} Hardware timestamps: {'available' if hw_ok else 'NOT available'}")
    print(f"  {status(jitter_ok)} Jitter: {stats.jitter_percent:.2f}% (< 10% required)")
    print(f"  {status(fps_ok)} FPS accuracy: {stats.actual_fps:.2f} / {stats.target_fps:.2f}")
    print(f"  {status(drops_ok)} Dropped frames: {stats.dropped_percent:.2f}% (< 1% required)")
    print(f"  {status(mono_ok)} Timestamps monotonic: {'yes' if mono_ok else 'NO'}")

    # Final verdict
    print()
    if stats.is_valid:
        print("RESULT: PASS - Camera timing is acceptable for scientific use")
    else:
        print("RESULT: FAIL - Camera timing does NOT meet requirements")
        for reason in stats.failure_reasons:
            print(f"        - {reason}")


def create_camera(
    backend: str,
    device_index: int,
    config: FrameConfig,
    target_fps: float,
    accept_mismatch: bool = False,
):
    """Create camera instance for the specified backend.

    Args:
        backend: Camera backend name (auto, avfoundation, opencv, pco)
        device_index: Camera device index
        config: Frame configuration
        target_fps: Target frame rate
        accept_mismatch: Accept format mismatches (for cameras that don't support selection)
    """

    if backend == "auto":
        # Auto-select based on platform
        if sys.platform == "darwin":
            backend = "avfoundation"
        else:
            backend = "pco"  # Scientific camera on non-macOS

    if backend == "avfoundation":
        if sys.platform != "darwin":
            print("ERROR: AVFoundation is only available on macOS")
            sys.exit(1)
        from .camera.avfoundation import AVFoundationCamera
        return AVFoundationCamera(
            config,
            device_index=device_index,
            target_fps=target_fps,
            accept_mismatch=accept_mismatch,
        )

    elif backend == "pco":
        if sys.platform == "darwin":
            print("ERROR: PCO cameras are not supported on macOS")
            sys.exit(1)
        from .camera.pco import PcoCamera
        return PcoCamera(config, camera_number=device_index)

    else:
        print(f"ERROR: Unknown camera backend: {backend}")
        sys.exit(1)


def list_cameras() -> None:
    """List available cameras."""
    print("=== Available Cameras ===\n")

    # Try to enumerate
    try:
        from .camera.enumerate import enumerate_all_cameras
        result = enumerate_all_cameras()

        if not result:
            print("No cameras found.")
            return

        # Result is a dict: {"avfoundation": {...}, "opencv": {...}, "pco": {...}}
        for backend, info in result.items():
            if not info.get("available", False):
                continue

            print(f"--- {backend.upper()} ---")
            devices = info.get("devices", [])

            if not devices:
                print("  No devices found")
                continue

            for dev in devices:
                if isinstance(dev, dict):
                    idx = dev.get("index", dev.get("device_index", "?"))
                    name = dev.get("name", "Unknown")
                    width = dev.get("width", "?")
                    height = dev.get("height", "?")
                    fps = dev.get("fps", "?")
                    print(f"  [{idx}] {name}")
                    print(f"       Resolution: {width}x{height} @ {fps} fps")
                else:
                    # String or other format
                    print(f"  {dev}")
            print()

    except Exception as e:
        print(f"Error enumerating cameras: {e}")

        # Fallback: try OpenCV directly
        print("\nTrying OpenCV enumeration...")
        try:
            from .camera.enumerate import enumerate_opencv_cameras
            cameras = enumerate_opencv_cameras()
            if cameras:
                for cam in cameras:
                    idx = cam.get("index", "?")
                    name = cam.get("name", "Unknown")
                    w = cam.get("width", "?")
                    h = cam.get("height", "?")
                    print(f"  [{idx}] {name} - {w}x{h}")
            else:
                print("  No cameras found")
        except Exception as e2:
            print(f"  Failed: {e2}")


def probe_camera_format(
    backend: str,
    device_index: int,
    fps: float,
) -> Optional[tuple[int, int, int]]:
    """Probe camera to discover its actual output format by capturing a test frame.

    Some cameras (e.g., FaceTime HD on Apple Silicon) don't support format
    selection and always output at a fixed resolution. This function discovers
    what the camera actually outputs by capturing real frames.

    Returns:
        Tuple of (width, height, bits_per_pixel) or None if probe fails
    """
    if backend == "auto":
        if sys.platform == "darwin":
            backend = "avfoundation"
        else:
            backend = "opencv"

    if backend == "avfoundation":
        if sys.platform != "darwin":
            return None

        # Enumerate cameras to find actual supported formats, then use the
        # highest resolution that supports the target fps. The camera will
        # output in this format (though it may not match FrameConfig).
        try:
            from .camera.avfoundation import enumerate_cameras

            cameras = enumerate_cameras()
            if device_index >= len(cameras):
                return None

            camera_info = cameras[device_index]
            formats = camera_info.get("formats", [])

            if not formats:
                return None

            # Find best format: highest resolution that supports target fps
            best_format = None
            for fmt in formats:
                min_fps = fmt.get("min_fps", 0)
                max_fps = fmt.get("max_fps", 0)
                if min_fps <= fps <= max_fps:
                    if best_format is None:
                        best_format = fmt
                    elif fmt["width"] * fmt["height"] > best_format["width"] * best_format["height"]:
                        best_format = fmt

            # Fall back to first format with any fps
            if best_format is None and formats:
                best_format = formats[0]

            if best_format:
                # YUV formats (2vuy, etc.) are 16 bpp, not what's in bits_per_pixel
                # The bits_per_pixel from enumeration is the sensor format, but
                # AVCaptureVideoDataOutput typically outputs YUV 4:2:2 (16 bpp)
                bpp = 16  # Default for YUV output from AVFoundation

                return (
                    best_format["width"],
                    best_format["height"],
                    bpp,
                )

            return None

        except Exception as e:
            print(f"  AVFoundation probe error: {e}")
            return None

    elif backend == "opencv":
        # OpenCV probe: connect briefly and check actual dimensions
        try:
            import cv2
            cap = cv2.VideoCapture(device_index)
            if cap.isOpened():
                width = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
                height = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
                cap.release()
                return (width, height, 24)  # OpenCV typically returns BGR
        except Exception:
            pass

    return None


def run_test(
    backend: str,
    device_index: int,
    width: int,
    height: int,
    fps: float,
    bits_per_pixel: int,
    duration_sec: float,
    auto_format: bool = False,
) -> int:
    """Run the camera timing test.

    Returns:
        Exit code (0 for pass, 1 for fail)
    """

    print("=== Camera Timing Test ===\n")
    print(f"Backend:    {backend}")
    print(f"Device:     {device_index}")

    # Auto-detect format if requested
    if auto_format:
        print("\nAuto-detecting camera format...")
        probed = probe_camera_format(backend, device_index, fps)
        if probed:
            width, height, bits_per_pixel = probed
            print(f"  Detected: {width}x{height} @ {bits_per_pixel} bpp")
        else:
            print("  Warning: Could not auto-detect format, using defaults")

    print(f"\nResolution: {width}x{height}")
    print(f"Bits/pixel: {bits_per_pixel}")
    print(f"Target FPS: {fps}")
    print(f"Duration:   {duration_sec} sec")
    print()

    # Create config
    config = FrameConfig(
        width=width,
        height=height,
        bits_per_pixel=bits_per_pixel,
    )

    # Create camera
    print("Creating camera...")
    camera = create_camera(backend, device_index, config, fps, accept_mismatch=auto_format)

    # Connect
    print("Connecting to camera...")
    if not camera.connect():
        print("ERROR: Failed to connect to camera")
        print("\nHint: Try --auto-format to auto-detect camera capabilities")
        return 1

    # Check capabilities
    caps = camera.capabilities
    print(f"\nCamera capabilities:")
    print(f"  Hardware timestamps: {caps.hardware_timestamps}")
    print(f"  Hardware trigger:    {caps.hardware_trigger}")
    print(f"  Hardware strobe:     {caps.hardware_strobe}")

    if not caps.hardware_timestamps:
        print("\nWARNING: Camera does not support hardware timestamps!")
        print("         Results will be SOFTWARE timing only - NOT scientifically valid.")

    # Start acquisition
    print("\nStarting acquisition...")
    if not camera.start_acquisition():
        print("ERROR: Failed to start acquisition")
        camera.disconnect()
        return 1

    # Capture frames
    print(f"Capturing for {duration_sec} seconds...")
    timestamps: list[int] = []
    frame_count = 0
    start_time = time.perf_counter()
    last_report = start_time

    while True:
        elapsed = time.perf_counter() - start_time
        if elapsed >= duration_sec:
            break

        # Get frame
        frame = camera.get_frame()
        if frame is not None:
            if frame.timestamp_us is not None:
                timestamps.append(frame.timestamp_us)
            frame_count += 1

        # Progress report every second
        if time.perf_counter() - last_report >= 1.0:
            print(f"\r  Captured {frame_count} frames...", end="", flush=True)
            last_report = time.perf_counter()

        # Small sleep to prevent 100% CPU
        time.sleep(0.0001)

    elapsed_sec = time.perf_counter() - start_time
    print(f"\r  Captured {frame_count} frames.      ")

    # Stop acquisition
    print("Stopping acquisition...")
    camera.stop_acquisition()
    camera.disconnect()

    # Check if we got timestamps
    if len(timestamps) == 0:
        print("\nERROR: No timestamps captured!")
        print("       Camera may not support hardware timestamps.")
        return 1

    if len(timestamps) < frame_count:
        print(f"\nWARNING: Only {len(timestamps)}/{frame_count} frames had timestamps")

    # Compute statistics
    stats = compute_stats(
        timestamps_us=timestamps,
        elapsed_sec=elapsed_sec,
        target_fps=fps,
        hardware_timestamps=caps.hardware_timestamps,
    )

    # Print results
    print_stats(stats)

    return 0 if stats.is_valid else 1


def main() -> int:
    """Main entry point."""

    parser = argparse.ArgumentParser(
        description="Test camera hardware timestamps and timing performance",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s                          # Test default camera
  %(prog)s --auto-format            # Auto-detect camera format
  %(prog)s --list                   # List available cameras
  %(prog)s --camera avfoundation    # Use AVFoundation (macOS)
  %(prog)s --duration 10 --fps 60   # 10 second test at 60 FPS
  %(prog)s --width 1920 --height 1080  # Full HD resolution
""",
    )

    parser.add_argument(
        "--list", "-l",
        action="store_true",
        help="List available cameras and exit",
    )

    parser.add_argument(
        "--camera", "-c",
        choices=["auto", "avfoundation", "pco"],
        default="auto",
        help="Camera backend (default: auto)",
    )

    parser.add_argument(
        "--device", "-d",
        type=int,
        default=0,
        help="Device index (default: 0)",
    )

    parser.add_argument(
        "--width", "-W",
        type=int,
        default=1280,
        help="Frame width (default: 1280)",
    )

    parser.add_argument(
        "--height", "-H",
        type=int,
        default=720,
        help="Frame height (default: 720)",
    )

    parser.add_argument(
        "--fps", "-f",
        type=float,
        default=30.0,
        help="Target frame rate (default: 30)",
    )

    parser.add_argument(
        "--bits-per-pixel", "-b",
        type=int,
        default=24,
        help="Bits per pixel (default: 24 for RGB)",
    )

    parser.add_argument(
        "--duration", "-t",
        type=float,
        default=5.0,
        help="Test duration in seconds (default: 5)",
    )

    parser.add_argument(
        "--auto-format", "-a",
        action="store_true",
        help="Auto-detect camera format (use when camera doesn't support format selection)",
    )

    args = parser.parse_args()

    if args.list:
        list_cameras()
        return 0

    return run_test(
        backend=args.camera,
        device_index=args.device,
        width=args.width,
        height=args.height,
        fps=args.fps,
        bits_per_pixel=args.bits_per_pixel,
        duration_sec=args.duration,
        auto_format=args.auto_format,
    )


if __name__ == "__main__":
    sys.exit(main())
