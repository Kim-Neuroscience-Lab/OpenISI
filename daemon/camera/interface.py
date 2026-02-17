"""Camera interface with high-performance capture architecture.

This module provides the Camera base class that implements optimized capture patterns:

1. **Worker thread architecture**: Lightweight callback, heavy work off critical path
2. **Lock-free queues**: Atomic deque operations (CPython GIL)
3. **Event signaling**: No polling, immediate wake-up
4. **Batch processing**: Process all queued frames per wake-up
5. **Zero-copy where possible**: Backend-specific buffer access

Backends only need to implement the camera-specific parts:
- Setting up capture session
- Starting/stopping hardware capture
- Copying pixel data (with backend-specific optimizations)
- Releasing raw buffers

The base class handles all threading, synchronization, and queue management.
"""

from abc import ABC, abstractmethod
from collections import deque
from dataclasses import dataclass
from enum import Enum
from typing import Any, Generic, TypeVar
import threading
import numpy as np

from ..config import FrameConfig


# Type variable for backend-specific raw buffer type
RawBuffer = TypeVar('RawBuffer')


@dataclass
class RawFrame(Generic[RawBuffer]):
    """Raw frame data from camera callback.

    This is what the callback enqueues. The worker thread processes it
    into a FrameResult.
    """
    buffer: RawBuffer           # Backend-specific buffer (retained/referenced)
    timestamp_us: int           # Hardware timestamp in microseconds
    pixel_format: int           # Pixel format code
    width: int                  # Frame width
    height: int                 # Frame height
    metadata: dict[str, Any] | None = None  # Optional backend-specific metadata


class SyncMode(Enum):
    """Synchronization mode between camera and stimulus.

    TRIGGERED: Hardware trigger signal enforces synchronization.
               System auto-selects master (stimulus or camera) based on
               capabilities and modality requirements.

    POST_HOC: Camera and stimulus run independently at their natural rates.
              Synchronization is established after acquisition by correlating
              hardware timestamps from both streams.
    """
    TRIGGERED = "triggered"
    POST_HOC = "post_hoc"


@dataclass(frozen=True)
class SyncConfig:
    """Synchronization configuration.

    Attributes:
        mode: The synchronization strategy (TRIGGERED or POST_HOC)
        trigger_delay_us: Delay between trigger event and action (TRIGGERED mode only)
    """
    mode: SyncMode = SyncMode.POST_HOC
    trigger_delay_us: int = 0

    def __post_init__(self):
        if self.mode == SyncMode.POST_HOC and self.trigger_delay_us != 0:
            raise ValueError("trigger_delay_us only applies to TRIGGERED mode")

    @classmethod
    def triggered(cls, delay_us: int = 0) -> 'SyncConfig':
        """Create a triggered sync configuration."""
        return cls(mode=SyncMode.TRIGGERED, trigger_delay_us=delay_us)

    @classmethod
    def post_hoc(cls) -> 'SyncConfig':
        """Create a post-hoc sync configuration."""
        return cls(mode=SyncMode.POST_HOC)


@dataclass(frozen=True)
class CameraCapabilities:
    """Capabilities that a camera implementation may support.

    Each camera probes its own capabilities at connection time.
    The acquisition system checks these against requirements.
    """
    hardware_timestamps: bool = False  # Can provide hardware capture timestamps
    hardware_trigger: bool = False     # Can accept external trigger input
    hardware_strobe: bool = False      # Can output strobe/sync signal

    def supports_sync_mode(self, mode: SyncMode) -> bool:
        """Check if camera supports a given sync mode."""
        if mode == SyncMode.POST_HOC:
            return self.hardware_timestamps
        elif mode == SyncMode.TRIGGERED:
            # Triggered mode requires timestamps AND (trigger input OR strobe output)
            return self.hardware_timestamps and (self.hardware_trigger or self.hardware_strobe)
        return False

    def best_sync_mode(self) -> SyncMode | None:
        """Determine the best sync mode this camera supports.

        Returns TRIGGERED if available (better precision), otherwise POST_HOC.
        Returns None if camera doesn't support any valid sync mode.
        """
        if self.supports_sync_mode(SyncMode.TRIGGERED):
            return SyncMode.TRIGGERED
        elif self.supports_sync_mode(SyncMode.POST_HOC):
            return SyncMode.POST_HOC
        return None

    def meets_requirements(self, requirements: 'AcquisitionRequirements') -> tuple[bool, list[str]]:
        """Check if capabilities meet acquisition requirements.

        Returns:
            (success, list of missing capabilities)
        """
        missing = []

        # Check hardware timestamps (always required for scientific use)
        if requirements.require_hardware_timestamps and not self.hardware_timestamps:
            missing.append("hardware_timestamps")

        # Check sync mode compatibility
        if not self.supports_sync_mode(requirements.sync.mode):
            if requirements.sync.mode == SyncMode.TRIGGERED:
                if not self.hardware_trigger and not self.hardware_strobe:
                    missing.append("hardware_trigger or hardware_strobe (for TRIGGERED sync)")
            elif requirements.sync.mode == SyncMode.POST_HOC:
                if not self.hardware_timestamps:
                    missing.append("hardware_timestamps (for POST_HOC sync)")

        return len(missing) == 0, missing


@dataclass(frozen=True)
class AcquisitionRequirements:
    """Requirements for a valid acquisition.

    Scientific acquisitions require hardware timestamps and specify sync mode.
    Development/testing may relax these requirements.
    """
    require_hardware_timestamps: bool = True
    sync: SyncConfig = None  # Will be set to POST_HOC by default in __post_init__

    def __post_init__(self):
        # Handle frozen dataclass default initialization
        if self.sync is None:
            object.__setattr__(self, 'sync', SyncConfig.post_hoc())

    @classmethod
    def scientific(cls, sync: SyncConfig | None = None) -> 'AcquisitionRequirements':
        """Requirements for scientifically valid acquisition.

        Args:
            sync: Sync configuration. If None, system will auto-select based on
                  camera capabilities (TRIGGERED if available, else POST_HOC).
        """
        return cls(
            require_hardware_timestamps=True,
            sync=sync or SyncConfig.post_hoc(),
        )

    @classmethod
    def scientific_auto_sync(cls, capabilities: 'CameraCapabilities') -> 'AcquisitionRequirements':
        """Create scientific requirements with auto-selected sync mode.

        Selects TRIGGERED if camera supports it, otherwise POST_HOC.
        """
        best_mode = capabilities.best_sync_mode()
        if best_mode is None:
            raise ValueError("Camera does not support any valid sync mode")

        sync = SyncConfig(mode=best_mode)
        return cls(
            require_hardware_timestamps=True,
            sync=sync,
        )

    @classmethod
    def development(cls) -> 'AcquisitionRequirements':
        """Relaxed requirements for development/testing."""
        return cls(
            require_hardware_timestamps=False,
            sync=SyncConfig.post_hoc(),
        )


@dataclass
class FrameResult:
    """Result of capturing a single frame.

    Contains pixel data and metadata including hardware timestamp.
    Hardware timestamps are REQUIRED - cameras without them cannot be used.
    """
    data: np.ndarray | list[np.ndarray]  # Pixel data (format depends on pixel_format)
    frame_index: int              # Sequential frame number from this acquisition
    timestamp_us: int             # Hardware timestamp in microseconds (REQUIRED)
    pixel_format: int             # Pixel format code (e.g., CoreVideo format for AVFoundation)

    # Optional extended metadata (camera-specific)
    exposure_us: int | None = None      # Actual exposure time if known
    sensor_temperature: float | None = None  # Sensor temp if available


class Camera(ABC):
    """Base class for high-performance camera implementations.

    This class implements the optimized capture architecture:

    ```
    Camera Hardware
         │
         ▼
    Backend Callback (hot path - must be FAST)
         │ - Extract timestamp
         │ - Retain buffer reference
         │ - Enqueue RawFrame (atomic, lock-free)
         │ - Signal event
         ▼
    Worker Thread (cold path - can be slow)
         │ - Wait on event
         │ - Dequeue all frames
         │ - Copy pixel data (backend-specific)
         │ - Release buffers
         │ - Enqueue FrameResult
         ▼
    Output Queue
         │
         ▼
    get_frame() → FrameResult
    ```

    Subclasses must implement:
    - _setup_capture(): Configure backend-specific capture
    - _teardown_capture(): Release backend resources
    - _start_capture(): Start hardware acquisition
    - _stop_capture(): Stop hardware acquisition
    - _copy_frame_data(): Copy pixels from raw buffer (with optimizations)
    - _release_buffer(): Release/free the raw buffer
    - _get_capabilities(): Return camera capabilities
    """

    # Configuration
    RAW_QUEUE_SIZE = 30      # ~1 second at 30fps
    OUTPUT_QUEUE_SIZE = 5    # Small output buffer

    def __init__(self, config: FrameConfig):
        self._config = config
        self._capabilities: CameraCapabilities | None = None

        # Raw frame queue: callback enqueues here (lock-free, atomic)
        self._raw_queue: deque[RawFrame] = deque(maxlen=self.RAW_QUEUE_SIZE)
        self._frame_available = threading.Event()

        # Output queue: worker enqueues processed frames
        self._output_queue: deque[FrameResult] = deque(maxlen=self.OUTPUT_QUEUE_SIZE)

        # Worker thread state
        self._worker_thread: threading.Thread | None = None
        self._stop_worker = threading.Event()

        # Acquisition state
        self._frame_index = 0
        self._acquiring = False
        self._connected = False

        # Mismatch handling
        self._accept_mismatch = False
        self._mismatch_logged = False

    # =========================================================================
    # Properties
    # =========================================================================

    @property
    def width(self) -> int:
        return self._config.width

    @property
    def height(self) -> int:
        return self._config.height

    @property
    def capabilities(self) -> CameraCapabilities:
        """Camera capabilities (available after connect)."""
        if self._capabilities is None:
            raise RuntimeError("Camera not connected - capabilities unknown")
        return self._capabilities

    @property
    def is_connected(self) -> bool:
        return self._connected

    @property
    def is_acquiring(self) -> bool:
        return self._acquiring

    @property
    def frame_config(self) -> FrameConfig:
        """Current frame configuration."""
        return self._config

    # =========================================================================
    # Abstract methods - backends must implement
    # =========================================================================

    @abstractmethod
    def _setup_capture(self) -> bool:
        """Set up the backend-specific capture pipeline.

        Called from connect(). Should:
        1. Open/initialize the camera device
        2. Configure capture format/resolution
        3. Set up callbacks that will call _enqueue_raw_frame()

        Returns:
            True if setup succeeded, False otherwise.
        """
        pass

    @abstractmethod
    def _teardown_capture(self) -> None:
        """Tear down the capture pipeline.

        Called from disconnect(). Should release all backend resources.
        """
        pass

    @abstractmethod
    def _start_capture(self) -> bool:
        """Start the hardware capture.

        Called from start_acquisition(). Should start the camera delivering
        frames to the callback.

        Returns:
            True if started successfully.
        """
        pass

    @abstractmethod
    def _stop_capture(self) -> None:
        """Stop the hardware capture.

        Called from stop_acquisition(). Should stop frame delivery.
        """
        pass

    @abstractmethod
    def _copy_frame_data(self, raw_frame: RawFrame) -> np.ndarray | list[np.ndarray] | None:
        """Copy pixel data from the raw buffer.

        This is where backend-specific optimizations go:
        - AVFoundation: Use objc.varlist.as_buffer() for zero-copy access
        - V4L2: Use mmap for zero-copy

        Args:
            raw_frame: The raw frame with buffer reference

        Returns:
            Copied numpy array(s), or None if copy failed.
        """
        pass

    @abstractmethod
    def _release_buffer(self, raw_frame: RawFrame) -> None:
        """Release the raw buffer back to the camera system.

        Called after _copy_frame_data() completes. Must be called even if
        copy failed.

        Args:
            raw_frame: The raw frame whose buffer should be released.
        """
        pass

    @abstractmethod
    def _get_capabilities(self) -> CameraCapabilities:
        """Get the camera's capabilities.

        Called during connect() after _setup_capture() succeeds.

        Returns:
            CameraCapabilities for this camera.
        """
        pass

    # =========================================================================
    # Callback interface - backends call this from their callbacks
    # =========================================================================

    def _enqueue_raw_frame(self, raw_frame: RawFrame) -> None:
        """Enqueue a raw frame from the backend callback.

        This method is designed to be called from the camera's callback
        and must be FAST. It only:
        1. Appends to deque (atomic in CPython)
        2. Signals the event (non-blocking)

        The backend callback should:
        1. Extract the hardware timestamp
        2. Retain/reference the buffer (backend-specific)
        3. Create a RawFrame with the info
        4. Call this method

        Args:
            raw_frame: Frame info with retained buffer reference.
        """
        # Atomic append (CPython GIL)
        self._raw_queue.append(raw_frame)
        # Signal worker (non-blocking)
        self._frame_available.set()

    # =========================================================================
    # Public interface
    # =========================================================================

    def connect(self) -> bool:
        """Connect to the camera and probe capabilities."""
        if self._connected:
            return True

        if not self._setup_capture():
            return False

        self._capabilities = self._get_capabilities()
        self._connected = True
        return True

    def disconnect(self) -> None:
        """Disconnect from the camera."""
        if not self._connected:
            return

        if self._acquiring:
            self.stop_acquisition()

        self._teardown_capture()
        self._connected = False
        self._capabilities = None

    def start_acquisition(self) -> bool:
        """Start acquiring frames."""
        if not self._connected:
            return False

        if self._acquiring:
            return True

        # Reset state
        self._frame_index = 0
        self._raw_queue.clear()
        self._output_queue.clear()
        self._mismatch_logged = False

        # Start worker thread first
        self._stop_worker.clear()
        self._frame_available.clear()
        self._worker_thread = threading.Thread(
            target=self._worker_loop,
            daemon=True,
            name=f"{self.__class__.__name__}-worker"
        )
        self._worker_thread.start()

        # Start hardware capture
        self._acquiring = True
        if not self._start_capture():
            self._acquiring = False
            self._stop_worker.set()
            self._frame_available.set()
            self._worker_thread.join(timeout=1.0)
            self._worker_thread = None
            return False

        return True

    def stop_acquisition(self) -> None:
        """Stop acquiring frames."""
        if not self._acquiring:
            return

        self._acquiring = False

        # Stop hardware first
        self._stop_capture()

        # Stop worker thread
        self._stop_worker.set()
        self._frame_available.set()  # Wake up for clean shutdown
        if self._worker_thread is not None:
            self._worker_thread.join(timeout=1.0)
            self._worker_thread = None

        # Release any remaining raw buffers
        while True:
            try:
                raw_frame = self._raw_queue.popleft()
                self._release_buffer(raw_frame)
            except IndexError:
                break

    def get_frame(self) -> FrameResult | None:
        """Get the next processed frame."""
        try:
            return self._output_queue.popleft()
        except IndexError:
            return None

    def check_requirements(self, requirements: AcquisitionRequirements) -> tuple[bool, list[str]]:
        """Check if this camera meets acquisition requirements.

        Args:
            requirements: The acquisition requirements to check against

        Returns:
            (success, list of missing capabilities)
        """
        return self.capabilities.meets_requirements(requirements)

    # =========================================================================
    # Worker thread
    # =========================================================================

    def _worker_loop(self) -> None:
        """Worker thread main loop.

        Waits for frames, processes them in batches, and enqueues results.
        """
        while not self._stop_worker.is_set():
            # Wait for frame signal or timeout
            self._frame_available.wait(timeout=0.1)
            self._frame_available.clear()

            # Process ALL available frames (batch processing)
            while True:
                try:
                    raw_frame = self._raw_queue.popleft()
                except IndexError:
                    break

                if self._stop_worker.is_set():
                    self._release_buffer(raw_frame)
                    return

                self._process_raw_frame(raw_frame)

    def _process_raw_frame(self, raw_frame: RawFrame) -> None:
        """Process a single raw frame into a FrameResult."""
        try:
            # Handle dimension mismatch if configured
            if (raw_frame.width != self._config.width or
                raw_frame.height != self._config.height):
                if self._accept_mismatch:
                    if not self._mismatch_logged:
                        print(f"{self.__class__.__name__}: Auto-adjusting config to "
                              f"{raw_frame.width}x{raw_frame.height}")
                        self._config = FrameConfig(
                            width=raw_frame.width,
                            height=raw_frame.height,
                            bits_per_pixel=self._config.bits_per_pixel,
                        )
                        self._mismatch_logged = True
                else:
                    raise ValueError(
                        f"Frame size mismatch: got {raw_frame.width}x{raw_frame.height}, "
                        f"expected {self._config.width}x{self._config.height}"
                    )

            # Copy pixel data (backend-specific optimization)
            frame_data = self._copy_frame_data(raw_frame)
            if frame_data is None:
                return

            # Create result
            result = FrameResult(
                data=frame_data,
                frame_index=self._frame_index,
                timestamp_us=raw_frame.timestamp_us,
                pixel_format=raw_frame.pixel_format,
            )

            self._frame_index += 1

            # Enqueue result (atomic)
            self._output_queue.append(result)

        except Exception as e:
            import traceback
            print(f"{self.__class__.__name__} worker error: {e}")
            traceback.print_exc()

        finally:
            # Always release the buffer
            self._release_buffer(raw_frame)
