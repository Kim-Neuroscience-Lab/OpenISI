class_name Protocol
extends RefCounted
## Shared memory protocol definitions (Single Source of Truth).
##
## These constants must match the Python daemon's protocol.py definitions.
## Any changes here must be reflected in both the Python daemon and Rust extension.

# --- Control Region Layout (64 bytes total) ---

## Size of the control region header in bytes.
const CONTROL_REGION_SIZE: int = 64

## Byte offsets for control region fields.
const OFFSET_WRITE_INDEX: int = 0     ## u32: Index of last written frame
const OFFSET_READ_INDEX: int = 4      ## u32: Index of last read frame (by consumer)
const OFFSET_FRAME_WIDTH: int = 8     ## u32: Frame width in pixels
const OFFSET_FRAME_HEIGHT: int = 12   ## u32: Frame height in pixels
const OFFSET_FRAME_COUNT: int = 16    ## u32: Total frames written (for drop detection)
const OFFSET_NUM_BUFFERS: int = 20    ## u32: Number of buffers in ring
const OFFSET_STATUS: int = 24         ## u8: Daemon status code
const OFFSET_COMMAND: int = 25        ## u8: Command from consumer to daemon
const OFFSET_RESERVED: int = 26       ## 38 bytes reserved for future use

# --- Status Codes ---

## Daemon status values (OFFSET_STATUS).
enum Status {
	IDLE = 0,         ## Daemon initialized but not acquiring
	ACQUIRING = 1,    ## Actively acquiring frames
	ERROR = 2,        ## Error state
	STOPPING = 3,     ## Graceful shutdown in progress
}

# --- Command Codes ---

## Consumer command values (OFFSET_COMMAND).
enum Command {
	NONE = 0,         ## No command pending
	START = 1,        ## Request to start acquisition
	STOP = 2,         ## Request to stop acquisition
	SHUTDOWN = 3,     ## Request daemon shutdown
}

# --- Frame Buffer Layout ---

## Bytes per pixel for raw frame data (uint16).
const BYTES_PER_PIXEL: int = 2

## Calculate the size of a single frame buffer in bytes.
static func frame_buffer_size(width: int, height: int) -> int:
	return width * height * BYTES_PER_PIXEL


## Calculate the total shared memory size needed.
static func total_shm_size(width: int, height: int, num_buffers: int) -> int:
	return CONTROL_REGION_SIZE + (frame_buffer_size(width, height) * num_buffers)


## Calculate the byte offset for a specific frame buffer.
static func frame_buffer_offset(buffer_index: int, width: int, height: int) -> int:
	return CONTROL_REGION_SIZE + (buffer_index * frame_buffer_size(width, height))


