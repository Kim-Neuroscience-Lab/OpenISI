"""Photodiode-based hardware timestamp capture for OpenISI.

Provides true hardware timestamps by detecting actual photon emission from
the display using a photodiode attached to a sync patch.

Supports any device following the OpenISI photodiode protocol:
- DIY Arduino + BPW34 photodiode (~$50)
- Stimulus Onset Hub (~$200)
- Black Box ToolKit (with adapter)
- Any device sending T<timestamp_us>\\n over USB serial
"""

from .reader import PhotodiodeReader, PhotodiodeEvent
from .correlator import correlate_timestamps

__all__ = ["PhotodiodeReader", "PhotodiodeEvent", "correlate_timestamps"]
