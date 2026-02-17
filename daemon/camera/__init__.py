"""Camera interfaces with high-performance capture architecture."""

import sys

from .interface import (
    Camera,
    RawFrame,
    FrameResult,
    CameraCapabilities,
    AcquisitionRequirements,
    SyncMode,
    SyncConfig,
)
from .enumerate import enumerate_all_cameras

# Platform-specific backends
if sys.platform == "darwin":
    # macOS: AVFoundation available
    from .avfoundation import (
        AVFoundationCamera,
        is_available as avfoundation_is_available,
        enumerate_cameras as enumerate_avfoundation_cameras,
    )
else:
    # Not on macOS: AVFoundation doesn't exist
    AVFoundationCamera = None
    avfoundation_is_available = lambda: False
    enumerate_avfoundation_cameras = lambda: []

if sys.platform in ("win32", "linux"):
    # Windows/Linux: PCO SDK available
    from .pco import PcoCamera, is_available as pco_is_available
else:
    # Not on Windows/Linux: PCO doesn't exist
    PcoCamera = None
    pco_is_available = lambda: False

# OpenCV backend - cross-platform fallback
from .opencv import (
    OpenCvCamera,
    is_available as opencv_is_available,
    enumerate_cameras as enumerate_opencv_cameras,
)

__all__ = [
    # Interface
    "Camera",
    "RawFrame",
    "FrameResult",
    "CameraCapabilities",
    "AcquisitionRequirements",
    "SyncMode",
    "SyncConfig",
    # Implementations
    "AVFoundationCamera",
    "avfoundation_is_available",
    "PcoCamera",
    "pco_is_available",
    "OpenCvCamera",
    "opencv_is_available",
    # Enumeration
    "enumerate_all_cameras",
    "enumerate_avfoundation_cameras",
    "enumerate_opencv_cameras",
]
