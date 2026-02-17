#!/usr/bin/env python3
"""OpenISI Camera Daemon.

This daemon captures frames from the camera and writes them to shared memory
for Godot to read. Timestamps are passed alongside frames for Godot to use
in its acquisition logging.

Usage:
    python -m daemon.main [--width WIDTH] [--height HEIGHT] [--fps FPS]
"""

import argparse
import os
import signal
import sys
import time
from typing import Optional

from .config import DaemonConfig, FrameConfig, SharedMemoryConfig
from .shm import SharedMemoryWriter
from .protocol import Status, Command
from .camera import (
    Camera,
    OpenCvCamera,
    AVFoundationCamera,
    avfoundation_is_available,
)


class Daemon:
    """Main daemon class."""

    def __init__(self, config: DaemonConfig):
        self._config = config
        self._running = False
        self._shm: Optional[SharedMemoryWriter] = None
        self._camera: Optional[Camera] = None

        # Statistics
        self._frames_written = 0
        self._start_time: Optional[float] = None

    def run(self) -> int:
        """Run the daemon. Returns exit code."""
        print("=" * 60)
        print("OpenISI Camera Daemon")
        print("=" * 60)
        print(f"Frame size: {self._config.frame.width}x{self._config.frame.height}")
        print(f"Target FPS: {self._config.target_fps}")
        print(f"Shared memory: {self._config.shm.name}")
        print(f"Camera type: {self._config.camera_type}")
        print("=" * 60)

        # Set up signal handlers
        signal.signal(signal.SIGINT, self._signal_handler)
        signal.signal(signal.SIGTERM, self._signal_handler)

        # Initialize and connect to camera FIRST (to verify actual output)
        self._camera = self._create_camera()
        if self._camera is None:
            print("ERROR: Failed to create camera")
            return 1

        if not self._camera.connect():
            print("ERROR: Failed to connect to camera")
            return 1

        # Initialize shared memory AFTER camera connects successfully
        self._shm = SharedMemoryWriter(self._config.frame, self._config.shm)
        if not self._shm.connect():
            print("ERROR: Failed to create shared memory")
            self._camera.disconnect()
            return 1

        # Print camera capabilities
        caps = self._camera.capabilities
        print(f"\nCamera capabilities:")
        print(f"  Hardware timestamps: {caps.hardware_timestamps}")
        print(f"  Hardware trigger: {caps.hardware_trigger}")
        print(f"  Hardware strobe: {caps.hardware_strobe}")

        # Start acquisition
        if not self._camera.start_acquisition():
            print("ERROR: Failed to start acquisition")
            self._camera.disconnect()
            self._shm.disconnect()
            return 1

        self._shm.set_status(Status.RUNNING)
        self._shm.set_daemon_pid(os.getpid())
        self._running = True
        self._start_time = time.perf_counter()

        print("\nDaemon running. Press Ctrl+C to stop.\n")

        # Main loop
        try:
            self._main_loop()
        except Exception as e:
            print(f"ERROR: {e}")
            import traceback
            traceback.print_exc()
            self._shm.set_status(Status.ERROR)
            return 1
        finally:
            self._cleanup()

        return 0

    def _create_camera(self) -> Optional[Camera]:
        """Create camera based on configuration."""
        camera_type = self._config.camera_type

        if camera_type == "opencv":
            return OpenCvCamera(self._config.frame, self._config.camera_device)

        elif camera_type == "avfoundation":
            if not avfoundation_is_available():
                print("ERROR: AVFoundation not available (macOS only)")
                return None
            return AVFoundationCamera(
                self._config.frame,
                self._config.camera_device,
                self._config.target_fps,
            )

        elif camera_type == "auto":
            # Auto-select best available camera with hardware timestamps
            if avfoundation_is_available():
                print("Auto-selected: AVFoundation (hardware timestamps)")
                return AVFoundationCamera(
                    self._config.frame,
                    self._config.camera_device,
                    self._config.target_fps,
                )
            else:
                print("Auto-selected: OpenCV (probing for hardware timestamps)")
                return OpenCvCamera(self._config.frame, self._config.camera_device)

        else:
            print(f"ERROR: Unknown camera type: {camera_type}")
            return None

    def _main_loop(self) -> None:
        """Main daemon loop."""
        last_stats_time = time.perf_counter()
        stats_interval = 2.0  # Print stats every 2 seconds

        while self._running:
            # Check for commands from client
            cmd = self._shm.get_command()
            if cmd == Command.STOP:
                print("Received STOP command from client")
                self._running = False
                break
            elif cmd != Command.NONE:
                self._shm.clear_command()

            # Get frame from camera
            frame = self._camera.get_frame()
            if frame is not None:
                # Write to shared memory with timestamp
                # Camera MUST provide hardware timestamps - no fallback
                assert frame.timestamp_us is not None, (
                    f"Camera {self._config.camera_type} returned frame without timestamp. "
                    "Hardware timestamps are required."
                )
                if self._shm.write_frame(frame.data, frame.timestamp_us):
                    self._frames_written += 1

            # Print periodic stats
            now = time.perf_counter()
            if now - last_stats_time >= stats_interval:
                self._print_stats()
                last_stats_time = now

            # Small sleep to prevent busy-waiting
            # The camera's get_frame() handles timing, this just prevents CPU spin
            time.sleep(0.001)

    def _print_stats(self) -> None:
        """Print current statistics."""
        if self._start_time is None:
            return

        elapsed = time.perf_counter() - self._start_time
        fps = self._frames_written / elapsed if elapsed > 0 else 0

        print(f"Frames: {self._frames_written:,} | "
              f"Elapsed: {elapsed:.1f}s | "
              f"FPS: {fps:.2f}")

    def _cleanup(self) -> None:
        """Clean up resources."""
        print("\nShutting down...")

        if self._camera is not None:
            self._camera.stop_acquisition()
            self._camera.disconnect()

        if self._shm is not None:
            self._shm.set_status(Status.STOPPED)
            self._shm.disconnect()

        # Final stats
        if self._start_time is not None:
            elapsed = time.perf_counter() - self._start_time
            fps = self._frames_written / elapsed if elapsed > 0 else 0
            print(f"\nFinal: {self._frames_written:,} frames in {elapsed:.1f}s ({fps:.2f} fps)")

    def _signal_handler(self, signum, frame) -> None:
        """Handle shutdown signals."""
        print(f"\nReceived signal {signum}")
        self._running = False


def parse_args() -> argparse.Namespace:
    """Parse command line arguments.

    All arguments are REQUIRED for daemon mode - no defaults.
    Values come from Godot's config system.

    --enumerate-cameras is a standalone mode that requires no other arguments.
    """
    parser = argparse.ArgumentParser(
        description="OpenISI Camera Daemon",
    )
    parser.add_argument(
        "--enumerate-cameras", action="store_true",
        help="Enumerate available cameras, print JSON to stdout, and exit"
    )
    parser.add_argument(
        "--width", type=int,
        help="Frame width in pixels"
    )
    parser.add_argument(
        "--height", type=int,
        help="Frame height in pixels"
    )
    parser.add_argument(
        "--fps", type=float,
        help="Target frames per second"
    )
    parser.add_argument(
        "--shm-name", type=str,
        help="Shared memory region name"
    )
    parser.add_argument(
        "--num-buffers", type=int,
        help="Number of frame buffers in ring buffer"
    )
    parser.add_argument(
        "--camera", type=str,
        choices=["opencv", "avfoundation", "auto"],
        help="Camera backend to use"
    )
    parser.add_argument(
        "--camera-device", type=int,
        help="Camera device index"
    )
    parser.add_argument(
        "--bits-per-pixel", type=int,
        help="Bits per pixel (e.g., 24 for BGR, 32 for BGRA)"
    )

    args = parser.parse_args()

    # In daemon mode, all arguments are required
    if not args.enumerate_cameras:
        required = ["width", "height", "fps", "shm_name", "num_buffers",
                     "camera", "camera_device", "bits_per_pixel"]
        missing = [name for name in required if getattr(args, name) is None]
        if missing:
            parser.error("the following arguments are required in daemon mode: "
                         + ", ".join("--" + name.replace("_", "-") for name in missing))

    return args


def main() -> int:
    """Entry point."""
    args = parse_args()

    # Enumerate cameras and exit immediately
    if args.enumerate_cameras:
        import json
        from .camera.enumerate import enumerate_all_cameras
        cameras = enumerate_all_cameras()
        print(json.dumps(cameras, indent=2))
        return 0

    config = DaemonConfig(
        frame=FrameConfig(
            width=args.width,
            height=args.height,
            bits_per_pixel=args.bits_per_pixel,
        ),
        shm=SharedMemoryConfig(name=args.shm_name, num_frames=args.num_buffers),
        target_fps=args.fps,
        camera_type=args.camera,
        camera_device=args.camera_device,
    )

    daemon = Daemon(config)
    return daemon.run()


if __name__ == "__main__":
    sys.exit(main())
