"""Shared memory writer for the daemon."""

import numpy as np
from multiprocessing import shared_memory
from typing import Optional

from .protocol import (
    CONTROL_REGION_SIZE,
    ControlRegion,
    Status,
    Command,
    calculate_shm_size,
)
from .config import FrameConfig, SharedMemoryConfig


class SharedMemoryWriter:
    """Writes frames to shared memory for Godot to read."""

    def __init__(self, frame_config: FrameConfig, shm_config: SharedMemoryConfig):
        self._frame_config = frame_config
        self._shm_config = shm_config
        self._shm: Optional[shared_memory.SharedMemory] = None
        self._control = ControlRegion(
            frame_width=frame_config.width,
            frame_height=frame_config.height,
            num_buffers=shm_config.num_frames,
        )

    @property
    def is_connected(self) -> bool:
        return self._shm is not None

    def connect(self) -> bool:
        """Create or connect to shared memory."""
        if self._shm is not None:
            return True

        size = calculate_shm_size(
            self._frame_config.width,
            self._frame_config.height,
            self._shm_config.num_frames,
            self._frame_config.bits_per_pixel,
        )

        try:
            # Try to create new shared memory
            self._shm = shared_memory.SharedMemory(
                name=self._shm_config.name,
                create=True,
                size=size,
            )
            print(f"Created shared memory '{self._shm_config.name}' ({size} bytes)")
        except FileExistsError:
            # Connect to existing
            try:
                self._shm = shared_memory.SharedMemory(
                    name=self._shm_config.name,
                    create=False,
                )
                print(f"Connected to existing shared memory '{self._shm_config.name}'")
            except FileNotFoundError:
                print(f"Failed to connect to shared memory '{self._shm_config.name}'")
                return False

        # Initialize control region
        self._write_control()
        return True

    def disconnect(self) -> None:
        """Disconnect from shared memory."""
        if self._shm is not None:
            try:
                self._shm.close()
                self._shm.unlink()
            except Exception as e:
                print(f"Error cleaning up shared memory: {e}")
            self._shm = None

    def write_frame(self, frame: np.ndarray | list[np.ndarray], timestamp_us: int) -> bool:
        """Write a frame to the ring buffer.

        Args:
            frame: The frame data - either a single numpy array (non-planar)
                   or a list of numpy arrays (planar formats like YUV420).
            timestamp_us: Hardware timestamp in microseconds (required).
        """
        if self._shm is None:
            return False

        # Convert frame data to bytes - pass through as-is from hardware
        if isinstance(frame, list):
            # Planar format: concatenate all planes
            frame_bytes = b''.join(plane.tobytes() for plane in frame)
        else:
            # Non-planar format: direct bytes
            frame_bytes = frame.tobytes()

        # Calculate buffer index and offset
        buffer_index = self._control.write_index % self._shm_config.num_frames
        frame_size = self._frame_config.frame_size_bytes
        frame_offset = CONTROL_REGION_SIZE + (buffer_index * frame_size)

        # Validate frame data size
        if len(frame_bytes) != frame_size:
            print(f"Frame size mismatch: expected {frame_size}, got {len(frame_bytes)}")
            return False

        # Write frame data
        self._shm.buf[frame_offset:frame_offset + frame_size] = frame_bytes

        # Update control region with timestamp
        self._control.write_index = (self._control.write_index + 1) % (2**32)
        self._control.frame_count += 1
        self._control.latest_timestamp_us = timestamp_us
        self._write_control()

        return True

    def set_status(self, status: Status) -> None:
        """Update daemon status."""
        self._control.status = status
        self._write_control()

    def set_daemon_pid(self, pid: int) -> None:
        """Store daemon PID in shared memory for cleanup."""
        self._control.daemon_pid = pid
        self._write_control()

    def get_command(self) -> Command:
        """Read command from client."""
        if self._shm is None:
            return Command.NONE
        self._read_control()
        return self._control.command

    def clear_command(self) -> None:
        """Clear the command after processing."""
        self._control.command = Command.NONE
        self._write_control()

    def _write_control(self) -> None:
        """Write control region to shared memory."""
        if self._shm is not None:
            self._shm.buf[:CONTROL_REGION_SIZE] = self._control.pack()

    def _read_control(self) -> None:
        """Read control region from shared memory (for command)."""
        if self._shm is not None:
            data = bytes(self._shm.buf[:CONTROL_REGION_SIZE])
            # Only update command field, preserve our write state
            temp = ControlRegion.unpack(data)
            self._control.command = temp.command
            self._control.read_index = temp.read_index

    def __enter__(self) -> "SharedMemoryWriter":
        self.connect()
        return self

    def __exit__(self, *args) -> None:
        self.disconnect()
