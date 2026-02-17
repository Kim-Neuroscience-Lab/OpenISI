"""Configuration for the daemon.

All values MUST be provided - no defaults. Values come from Godot's config system.
"""

from dataclasses import dataclass


@dataclass
class FrameConfig:
    """Frame configuration - all fields required."""
    width: int
    height: int
    bits_per_pixel: int  # Determined by camera hardware (e.g., 24 for BGR, 32 for BGRA)

    @property
    def frame_size_bytes(self) -> int:
        """Calculate frame size in bytes.

        Handles non-integer bytes per pixel (e.g., YUV420 is 12 bits = 1.5 bytes).
        Calculates total bits then rounds up to next byte.
        """
        total_bits = self.width * self.height * self.bits_per_pixel
        return (total_bits + 7) // 8  # Round up to next byte


@dataclass
class SharedMemoryConfig:
    """Shared memory configuration - all fields required."""
    name: str
    num_frames: int


@dataclass
class DaemonConfig:
    """Full daemon configuration - all fields required."""
    frame: FrameConfig
    shm: SharedMemoryConfig
    target_fps: float
    camera_type: str
    camera_device: int
