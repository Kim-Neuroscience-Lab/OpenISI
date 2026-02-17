"""PCO camera backend using the official PCO SDK.

Requires the `pco` package: pip install pco
Only supported on Windows and Linux (not macOS).

PCO cameras provide hardware timestamps in frame metadata.
"""

import sys
import numpy as np

from .interface import Camera, FrameResult, CameraCapabilities
from ..config import FrameConfig

# Check platform - PCO SDK only supports Windows and Linux
_PCO_SUPPORTED_PLATFORM = sys.platform in ("win32", "linux")

# Try to import pco SDK
_pco_available = False
_pco_import_error = None

if _PCO_SUPPORTED_PLATFORM:
    try:
        import pco
        _pco_available = True
    except ImportError as e:
        _pco_import_error = str(e)
    except Exception as e:
        _pco_import_error = f"Failed to load PCO SDK: {e}"


def is_available() -> bool:
    """Check if PCO SDK is available on this system."""
    return _pco_available


def get_import_error() -> str | None:
    """Get the import error message if PCO SDK failed to load."""
    if not _PCO_SUPPORTED_PLATFORM:
        return f"PCO SDK not supported on {sys.platform} (only Windows and Linux)"
    return _pco_import_error


def enumerate_cameras() -> list[dict]:
    """Enumerate available PCO cameras.

    Returns:
        List of dicts with camera info: {"index": int, "name": str, "serial": str}
    """
    if not _pco_available:
        return []

    cameras = []
    try:
        # PCO SDK allows opening cameras by index
        # Try to detect how many cameras are available
        for i in range(4):  # Check up to 4 cameras
            try:
                cam = pco.Camera(interface="USB 3.0", camera_number=i)
                info = {
                    "index": i,
                    "name": cam.camera_name if hasattr(cam, 'camera_name') else f"PCO Camera {i}",
                    "serial": cam.camera_serial if hasattr(cam, 'camera_serial') else "Unknown",
                }
                cam.close()
                cameras.append(info)
            except Exception:
                # No camera at this index
                break
    except Exception as e:
        print(f"PCO enumeration error: {e}")

    return cameras


class PcoCamera(Camera):
    """Camera using the PCO SDK.

    Works with PCO sCMOS cameras (Panda, Edge, etc.).
    PCO cameras always provide hardware timestamps via frame metadata.
    """

    def __init__(self, frame_config: FrameConfig, camera_number: int = 0):
        """Initialize PCO camera.

        Args:
            frame_config: Frame dimensions configuration
            camera_number: Camera index (0 = first PCO camera)
        """
        super().__init__(frame_config)
        self._camera_number = camera_number
        self._cam = None
        self._acquiring = False
        self._frame_index = 0

        if not _pco_available:
            error = get_import_error()
            raise RuntimeError(f"PCO SDK not available: {error}")

    def connect(self) -> bool:
        """Connect to the PCO camera."""
        if self._cam is not None:
            return True

        try:
            # Open camera - USB 3.0 interface
            self._cam = pco.Camera(interface="USB 3.0", camera_number=self._camera_number)

            # Configure camera
            self._cam.sdk.set_roi(1, 1, self._config.width, self._config.height)

            # Set exposure time if specified in config
            # Default to 33ms (30fps equivalent)
            exposure_ms = getattr(self._config, 'exposure_ms', 33.0)
            self._cam.sdk.set_delay_exposure_time(0, 'ms', exposure_ms, 'ms')

            # Arm the camera (prepare for acquisition)
            self._cam.sdk.arm_camera()

            print(f"PcoCamera connected: camera {self._camera_number}")
            if hasattr(self._cam, 'camera_name'):
                print(f"  Name: {self._cam.camera_name}")
            if hasattr(self._cam, 'camera_serial'):
                print(f"  Serial: {self._cam.camera_serial}")
            print(f"  ROI: {self._config.width}x{self._config.height}")

            # PCO cameras always support hardware timestamps
            self._capabilities = CameraCapabilities(
                hardware_timestamps=True,
                hardware_trigger=True,
                hardware_strobe=True,
            )

            return True

        except Exception as e:
            print(f"PcoCamera: Failed to connect: {e}")
            self._cam = None
            return False

    def disconnect(self) -> None:
        """Disconnect from the PCO camera."""
        if self._cam is not None:
            try:
                if self._acquiring:
                    self.stop_acquisition()
                self._cam.close()
            except Exception as e:
                print(f"PcoCamera: Error during disconnect: {e}")
            finally:
                self._cam = None
                self._capabilities = None
            print("PcoCamera disconnected")

    def start_acquisition(self) -> bool:
        """Start acquiring frames."""
        if self._cam is None:
            return False

        try:
            self._cam.sdk.set_recording_state(1)
            self._acquiring = True
            self._frame_index = 0
            print("PcoCamera acquisition started")
            return True
        except Exception as e:
            print(f"PcoCamera: Failed to start acquisition: {e}")
            return False

    def stop_acquisition(self) -> None:
        """Stop acquiring frames."""
        if self._cam is None:
            return

        try:
            self._cam.sdk.set_recording_state(0)
        except Exception as e:
            print(f"PcoCamera: Error stopping acquisition: {e}")

        self._acquiring = False
        print(f"PcoCamera acquisition stopped after {self._frame_index} frames")

    @property
    def is_connected(self) -> bool:
        """Whether the camera is connected."""
        return self._cam is not None

    @property
    def is_acquiring(self) -> bool:
        """Whether the camera is currently acquiring."""
        return self._acquiring

    def _extract_timestamp_us(self, metadata: dict) -> int:
        """Extract hardware timestamp from PCO metadata.

        PCO cameras embed timestamps in the image metadata.
        The format depends on the camera model and SDK version.

        Args:
            metadata: Metadata dict from pco.sdk.get_image()

        Returns:
            Timestamp in microseconds
        """
        # Try common metadata keys
        if 'timestamp' in metadata:
            ts = metadata['timestamp']
            # PCO timestamps are often in 100ns units
            if isinstance(ts, (int, float)):
                return int(ts / 10)  # Convert 100ns to us

        if 'time stamp' in metadata:
            ts = metadata['time stamp']
            if isinstance(ts, dict):
                # Some PCO cameras return structured timestamp
                if 'us' in ts:
                    return int(ts.get('us', 0))

        # If we can't find timestamp, this is a problem
        raise RuntimeError(
            f"Could not extract timestamp from PCO metadata. "
            f"Available keys: {list(metadata.keys())}"
        )

    def get_frame(self) -> FrameResult | None:
        """Get the next frame from the camera.

        Returns:
            FrameResult with grayscale uint16 frame and hardware timestamp,
            or None if no frame available.
        """
        if self._cam is None or not self._acquiring:
            return None

        try:
            image, metadata = self._cam.sdk.get_image()

            if image is None:
                return None

            # Extract hardware timestamp from metadata
            timestamp_us = self._extract_timestamp_us(metadata)

            # Ensure correct shape
            if image.shape != (self._config.height, self._config.width):
                import cv2
                image = cv2.resize(image, (self._config.width, self._config.height))

            # Ensure uint16
            if image.dtype != np.uint16:
                image = image.astype(np.uint16)

            result = FrameResult(
                data=image,
                frame_index=self._frame_index,
                timestamp_us=timestamp_us,
            )

            self._frame_index += 1
            return result

        except Exception as e:
            print(f"PcoCamera: Error getting frame: {e}")
            return None

    @property
    def frame_config(self) -> FrameConfig:
        """Get the frame configuration."""
        return self._config

    def set_exposure(self, exposure_ms: float) -> bool:
        """Set the exposure time in milliseconds."""
        if self._cam is None:
            return False

        try:
            self._cam.sdk.set_delay_exposure_time(0, 'ms', exposure_ms, 'ms')
            self._cam.sdk.arm_camera()
            print(f"PcoCamera: Exposure set to {exposure_ms}ms")
            return True
        except Exception as e:
            print(f"PcoCamera: Failed to set exposure: {e}")
            return False
