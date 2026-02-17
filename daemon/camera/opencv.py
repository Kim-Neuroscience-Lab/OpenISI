"""OpenCV camera backend - cross-platform fallback.

Uses opencv-python for basic camera capture. Does not support hardware timestamps.
"""

import time
from typing import Optional

import cv2
import numpy as np

from .interface import (
    Camera,
    RawFrame,
    FrameResult,
    CameraCapabilities,
    AcquisitionRequirements,
    SyncMode,
    SyncConfig,
)
from ..config import FrameConfig


def is_available() -> bool:
    """Check if OpenCV camera backend is available."""
    return cv2 is not None


def enumerate_cameras() -> list[dict]:
    """Enumerate available OpenCV cameras.

    OpenCV doesn't have good enumeration - we just probe indices.
    """
    cameras = []
    for i in range(4):  # Check first 4 indices
        cap = cv2.VideoCapture(i)
        if cap.isOpened():
            cameras.append({
                "index": i,
                "name": f"Camera {i}",
                "backend": "opencv",
            })
            cap.release()
    return cameras


class OpenCvCamera(Camera):
    """OpenCV camera backend.

    Cross-platform fallback that works on any system with a camera.
    Does NOT provide hardware timestamps - uses software timestamps only.
    """

    def __init__(self, frame_config: FrameConfig, device_index: int = 0):
        self._frame_config = frame_config
        self._device_index = device_index
        self._cap: Optional[cv2.VideoCapture] = None
        self._frame_count = 0

    def get_capabilities(self) -> CameraCapabilities:
        """Return camera capabilities."""
        return CameraCapabilities(
            supports_hardware_timestamps=False,
            supports_external_trigger=False,
            supports_frame_rate_control=True,
            min_frame_rate=1.0,
            max_frame_rate=120.0,
            available_sync_modes=[SyncMode.FREE_RUN],
        )

    def configure(self, requirements: AcquisitionRequirements) -> bool:
        """Configure camera for acquisition."""
        if self._cap is None:
            return False

        # Set resolution
        self._cap.set(cv2.CAP_PROP_FRAME_WIDTH, self._frame_config.width)
        self._cap.set(cv2.CAP_PROP_FRAME_HEIGHT, self._frame_config.height)

        # Set frame rate if specified
        if requirements.target_frame_rate > 0:
            self._cap.set(cv2.CAP_PROP_FPS, requirements.target_frame_rate)

        return True

    def start(self) -> bool:
        """Start camera capture."""
        self._cap = cv2.VideoCapture(self._device_index)
        if not self._cap.isOpened():
            print(f"OpenCvCamera: Failed to open device {self._device_index}")
            return False

        # Configure resolution
        self._cap.set(cv2.CAP_PROP_FRAME_WIDTH, self._frame_config.width)
        self._cap.set(cv2.CAP_PROP_FRAME_HEIGHT, self._frame_config.height)

        self._frame_count = 0
        print(f"OpenCvCamera: Started capture on device {self._device_index}")
        return True

    def stop(self) -> None:
        """Stop camera capture."""
        if self._cap is not None:
            self._cap.release()
            self._cap = None
        print("OpenCvCamera: Stopped capture")

    def get_frame(self, timeout_ms: int = 1000) -> FrameResult:
        """Get next frame from camera."""
        if self._cap is None or not self._cap.isOpened():
            return FrameResult(success=False, error="Camera not started")

        ret, frame = self._cap.read()
        if not ret:
            return FrameResult(success=False, error="Failed to read frame")

        # Convert to grayscale if needed
        if len(frame.shape) == 3:
            frame = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)

        # Resize if needed
        if frame.shape != (self._frame_config.height, self._frame_config.width):
            frame = cv2.resize(
                frame,
                (self._frame_config.width, self._frame_config.height),
                interpolation=cv2.INTER_LINEAR,
            )

        # Convert to 16-bit
        frame_16bit = (frame.astype(np.uint16) * 257)  # Scale 8-bit to 16-bit

        self._frame_count += 1

        # Software timestamp (microseconds since epoch)
        timestamp_us = int(time.time() * 1_000_000)

        raw_frame = RawFrame(
            data=frame_16bit,
            timestamp_us=timestamp_us,
            frame_number=self._frame_count,
            width=self._frame_config.width,
            height=self._frame_config.height,
        )

        return FrameResult(success=True, frame=raw_frame)

    def get_sync_config(self) -> SyncConfig:
        """Get current sync configuration."""
        return SyncConfig(mode=SyncMode.FREE_RUN)

    def set_sync_config(self, config: SyncConfig) -> bool:
        """Set sync configuration. OpenCV only supports free-run."""
        if config.mode != SyncMode.FREE_RUN:
            print(f"OpenCvCamera: Sync mode {config.mode} not supported, using FREE_RUN")
        return True
