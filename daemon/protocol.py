"""Shared memory protocol definitions.

This file defines the layout of the shared memory region. Both Python and Rust
must agree on this layout exactly.

Layout:
┌─────────────────────────────────────────────────────────────────┐
│ Control Region (64 bytes, fixed)                                │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │ Offset 0:  write_index   (u32) - Next frame to write        │ │
│ │ Offset 4:  read_index    (u32) - Last frame read by client  │ │
│ │ Offset 8:  frame_width   (u32) - Frame width in pixels      │ │
│ │ Offset 12: frame_height  (u32) - Frame height in pixels     │ │
│ │ Offset 16: frame_count   (u32) - Total frames written       │ │
│ │ Offset 20: num_buffers   (u32) - Number of frame buffers    │ │
│ │ Offset 24: status        (u8)  - Daemon status              │ │
│ │ Offset 25: command       (u8)  - Command from client        │ │
│ │ Offset 26: latest_timestamp_us (u64) - Hardware timestamp   │ │
│ │ Offset 34: daemon_pid    (u32) - Daemon process ID          │ │
│ │ Offset 38: reserved      (26 bytes)                         │ │
│ └─────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│ Frame Ring Buffer                                               │
│ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐           │
│ │ Frame 0  │ │ Frame 1  │ │ Frame 2  │ │ Frame 3  │           │
│ │ W×H×2    │ │ W×H×2    │ │ W×H×2    │ │ W×H×2    │           │
│ └──────────┘ └──────────┘ └──────────┘ └──────────┘           │
└─────────────────────────────────────────────────────────────────┘
"""

import struct
from dataclasses import dataclass
from enum import IntEnum


# Control region size (padded to 64 bytes for alignment)
CONTROL_REGION_SIZE = 64


class Status(IntEnum):
    """Daemon status codes."""
    STOPPED = 0
    RUNNING = 1
    ERROR = 2


class Command(IntEnum):
    """Commands from client to daemon."""
    NONE = 0
    START = 1
    STOP = 2


@dataclass
class ControlRegion:
    """Represents the control region of shared memory."""
    write_index: int = 0
    read_index: int = 0
    frame_width: int = 0
    frame_height: int = 0
    frame_count: int = 0
    num_buffers: int = 0
    status: Status = Status.STOPPED
    command: Command = Command.NONE
    latest_timestamp_us: int = 0  # Hardware timestamp of most recent frame
    daemon_pid: int = 0  # Daemon process ID for cleanup

    # Struct format: 6 u32s + 2 u8s + 1 u64 + 1 u32 + 26 bytes padding = 64 bytes
    _FORMAT = "<IIIIIIBBQI26x"

    def pack(self) -> bytes:
        """Pack control region to bytes."""
        return struct.pack(
            self._FORMAT,
            self.write_index,
            self.read_index,
            self.frame_width,
            self.frame_height,
            self.frame_count,
            self.num_buffers,
            self.status,
            self.command,
            self.latest_timestamp_us,
            self.daemon_pid,
        )

    @classmethod
    def unpack(cls, data: bytes) -> "ControlRegion":
        """Unpack control region from bytes."""
        values = struct.unpack(cls._FORMAT, data[:CONTROL_REGION_SIZE])
        return cls(
            write_index=values[0],
            read_index=values[1],
            frame_width=values[2],
            frame_height=values[3],
            frame_count=values[4],
            num_buffers=values[5],
            status=Status(values[6]),
            command=Command(values[7]),
            latest_timestamp_us=values[8],
            daemon_pid=values[9],
        )


def calculate_shm_size(frame_width: int, frame_height: int, num_buffers: int, bits_per_pixel: int) -> int:
    """Calculate total shared memory size needed.

    Args:
        frame_width: Frame width in pixels
        frame_height: Frame height in pixels
        num_buffers: Number of frame buffers in ring buffer
        bits_per_pixel: Bits per pixel (from hardware detection)
    """
    # For non-integer bytes per pixel (e.g., 12 bits for YUV420), calculate total bytes
    # YUV420: 12 bits/pixel = 1.5 bytes/pixel = (width * height * 12) / 8
    total_bits = frame_width * frame_height * bits_per_pixel
    frame_size = (total_bits + 7) // 8  # Round up to next byte
    return CONTROL_REGION_SIZE + (frame_size * num_buffers)
