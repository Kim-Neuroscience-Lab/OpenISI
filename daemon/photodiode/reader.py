"""Generic photodiode timestamp reader via USB serial.

Supports any device that sends: T<timestamp_us>\\n
Works with: DIY Arduino, Stimulus Onset Hub, Black Box ToolKit (with adapter)

Protocol:
    Device → Host:
        T<us>\\n     Timestamp in device microseconds
        S<code>\\n   Status (0=ok, 1=error)
        I<text>\\n   Device info

    Host → Device:
        P\\n         Ping (request timestamp for clock sync)
        H<val>\\n    Set threshold (0-1023)
        R\\n         Reset
"""

import serial
import serial.tools.list_ports
import threading
import time
from collections import deque
from dataclasses import dataclass
from typing import Optional


@dataclass
class PhotodiodeEvent:
    """A single photodiode detection event."""

    device_us: int  # Timestamp from device clock (microseconds since power-on)
    system_ns: int  # System CLOCK_MONOTONIC when serial data received
    corrected_us: int  # Timestamp converted to system timebase


class PhotodiodeReader:
    """Generic photodiode reader supporting any OpenISI-compatible device.

    Usage:
        reader = PhotodiodeReader()  # Auto-detect device
        reader.start()

        # ... run experiment ...

        timestamps = reader.get_timestamps_us()
        reader.stop()
    """

    # Buffer size for events (enough for ~30 minutes at 60Hz)
    MAX_EVENTS = 100_000

    def __init__(self, port: Optional[str] = None, baudrate: int = 115200):
        """Initialize photodiode reader.

        Args:
            port: Serial port (e.g., "/dev/tty.usbmodem1234"). Auto-detects if None.
            baudrate: Serial baud rate. Default 115200.
        """
        self.port = port
        self.baudrate = baudrate
        self.serial: Optional[serial.Serial] = None
        self.events: deque[PhotodiodeEvent] = deque(maxlen=self.MAX_EVENTS)
        self.running = False
        self._thread: Optional[threading.Thread] = None
        self._lock = threading.Lock()

        # Clock synchronization
        self._clock_offset_us: int = 0
        self._sync_samples: list[tuple[int, int]] = []  # (device_us, system_us) pairs

        # Device info
        self.device_name: Optional[str] = None

    @classmethod
    def list_devices(cls) -> list[str]:
        """List available USB serial devices that may be photodiode readers.

        Returns:
            List of port names (e.g., ["/dev/tty.usbmodem1234"])
        """
        ports = serial.tools.list_ports.comports()
        candidates = []
        for p in ports:
            # Filter for likely Arduino/photodiode devices
            if any(
                x in p.device.lower()
                for x in ["usbmodem", "usbserial", "acm", "arduino", "ch340"]
            ):
                candidates.append(p.device)
        return candidates

    def start(self) -> bool:
        """Start reading photodiode events.

        Returns:
            True if started successfully, False otherwise.
        """
        if self.running:
            return True

        # Auto-detect port if not specified
        if self.port is None:
            devices = self.list_devices()
            if not devices:
                print("PhotodiodeReader: No USB serial device found")
                return False
            self.port = devices[0]
            print(f"PhotodiodeReader: Auto-detected device at {self.port}")

        try:
            self.serial = serial.Serial(self.port, self.baudrate, timeout=0.1)

            # Arduino resets when serial opens - wait for it
            time.sleep(2.0)
            self.serial.reset_input_buffer()

            self.running = True
            self._thread = threading.Thread(target=self._read_loop, daemon=True)
            self._thread.start()

            # Perform initial clock synchronization
            self._synchronize_clocks()

            print(f"PhotodiodeReader: Started on {self.port}")
            return True

        except serial.SerialException as e:
            print(f"PhotodiodeReader: Failed to open {self.port}: {e}")
            return False

    def _read_loop(self):
        """Background thread that reads serial data."""
        while self.running:
            try:
                line = self.serial.readline()
                if not line:
                    continue

                # Record system time immediately
                system_ns = time.clock_gettime_ns(time.CLOCK_MONOTONIC)

                # Parse message
                try:
                    text = line.decode("ascii", errors="ignore").strip()
                except UnicodeDecodeError:
                    continue

                if not text:
                    continue

                msg_type = text[0]
                msg_data = text[1:]

                if msg_type == "T":
                    # Timestamp event
                    try:
                        device_us = int(msg_data)
                    except ValueError:
                        continue

                    # Convert to system timebase
                    corrected_us = device_us + self._clock_offset_us

                    event = PhotodiodeEvent(
                        device_us=device_us,
                        system_ns=system_ns,
                        corrected_us=corrected_us,
                    )

                    with self._lock:
                        self.events.append(event)

                elif msg_type == "I":
                    # Device info
                    self.device_name = msg_data
                    print(f"PhotodiodeReader: Device info: {msg_data}")

                elif msg_type == "S":
                    # Status
                    if msg_data != "0":
                        print(f"PhotodiodeReader: Device status: {msg_data}")

            except serial.SerialException:
                if self.running:
                    print("PhotodiodeReader: Serial error, stopping")
                    self.running = False
                break

    def _synchronize_clocks(self):
        """Synchronize device clock with system clock.

        The device uses micros() which starts at power-on (arbitrary epoch).
        We need to find the offset to convert to system CLOCK_MONOTONIC.

        Uses ping-pong protocol:
        1. Send 'P' to device
        2. Device responds with T<timestamp>
        3. Measure round-trip time
        4. Compute offset as: system_time - device_time - RTT/2
        """
        if not self.serial:
            return

        samples = []
        for _ in range(5):
            try:
                # Clear buffer
                self.serial.reset_input_buffer()

                # Send ping
                t1_ns = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
                self.serial.write(b"P\n")
                self.serial.flush()

                # Wait for response
                line = self.serial.readline()
                t2_ns = time.clock_gettime_ns(time.CLOCK_MONOTONIC)

                if line and line.startswith(b"T"):
                    device_us = int(line[1:].decode().strip())
                    rtt_ns = t2_ns - t1_ns
                    system_us = (t1_ns + rtt_ns // 2) // 1000  # Midpoint in us

                    offset = system_us - device_us
                    samples.append(offset)

            except (serial.SerialException, ValueError):
                continue

            time.sleep(0.05)  # Brief delay between samples

        if samples:
            # Use median to reject outliers
            samples.sort()
            self._clock_offset_us = samples[len(samples) // 2]
            print(f"PhotodiodeReader: Clock offset: {self._clock_offset_us} us")
        else:
            # Fallback: compute on first event
            print("PhotodiodeReader: Clock sync failed, will sync on first event")

    def get_events(self) -> list[PhotodiodeEvent]:
        """Get all captured photodiode events.

        Returns:
            List of PhotodiodeEvent objects.
        """
        with self._lock:
            return list(self.events)

    def get_timestamps_us(self) -> list[int]:
        """Get corrected timestamps in microseconds (system timebase).

        Returns:
            List of timestamps in microseconds since system boot.
        """
        with self._lock:
            return [e.corrected_us for e in self.events]

    def clear(self):
        """Clear all captured events."""
        with self._lock:
            self.events.clear()

    def set_threshold(self, value: int):
        """Set photodiode detection threshold.

        Args:
            value: Threshold value (0-1023 for 10-bit ADC)
        """
        if self.serial and 0 <= value <= 1023:
            self.serial.write(f"H{value}\n".encode())
            self.serial.flush()

    def stop(self):
        """Stop reading photodiode events."""
        if not self.running:
            return

        self.running = False

        if self._thread:
            self._thread.join(timeout=1.0)
            self._thread = None

        if self.serial:
            self.serial.close()
            self.serial = None

        print(f"PhotodiodeReader: Stopped, captured {len(self.events)} events")

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *args):
        self.stop()
