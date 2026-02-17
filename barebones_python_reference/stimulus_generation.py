"""
Stimulus Generation - Spherical checkerboard with counter-phase strobing.

Generates stimulus library for intrinsic signal imaging retinotopic mapping.
Based on Marshel et al. (2011) methods.
"""

import torch
import numpy as np
import h5py
import json
from pathlib import Path
from dataclasses import dataclass, asdict
from typing import Optional, Tuple
import logging

logging.basicConfig(level=logging.INFO, format='%(message)s')


def detect_monitor(monitor_index: int = 1) -> dict:
    """
    Auto-detect monitor properties using GLFW.

    Args:
        monitor_index: Monitor index (0 = primary, 1 = secondary/stimulus)

    Returns:
        Dictionary with monitor properties:
        - width_px, height_px: Resolution in pixels
        - width_cm, height_cm: Physical dimensions in cm
        - refresh_rate: Refresh rate in Hz
        - name: Monitor name
    """
    import glfw

    if not glfw.init():
        raise RuntimeError("Failed to initialize GLFW for monitor detection")

    try:
        monitors = glfw.get_monitors()
        if monitor_index >= len(monitors):
            raise ValueError(f"Monitor {monitor_index} not found. Only {len(monitors)} monitor(s) available.")

        monitor = monitors[monitor_index]
        mode = glfw.get_video_mode(monitor)
        phys_mm = glfw.get_monitor_physical_size(monitor)
        name = glfw.get_monitor_name(monitor)

        return {
            'width_px': mode.size.width,
            'height_px': mode.size.height,
            'width_cm': phys_mm[0] / 10.0,
            'height_cm': phys_mm[1] / 10.0,
            'refresh_rate': mode.refresh_rate,
            'name': name.decode() if isinstance(name, bytes) else name
        }
    finally:
        glfw.terminate()


@dataclass
class StimulusParams:
    """
    Stimulus generation parameters.

    Monitor properties (width/height in cm/px, fps) are auto-detected.
    Use StimulusParams.from_monitor() to create instances.
    """
    # Monitor geometry - AUTO-DETECTED, no defaults
    monitor_width_cm: float = None
    monitor_height_cm: float = None
    monitor_distance_cm: float = 10.0  # User must measure this
    monitor_width_px: int = None
    monitor_height_px: int = None

    # Visual angles (monitor positioning)
    angle_horizontal_deg: float = 20.0  # Tilt angle
    angle_vertical_deg: float = 30.0    # Lateral angle

    # Stimulus parameters
    stimulus_width_deg: float = 20.0
    checker_size_deg: float = 25.0
    sweep_speed_deg_per_sec: float = 9.0
    strobe_frequency_hz: float = 6.0

    # Display - AUTO-DETECTED
    display_fps: float = None
    scale_factor: float = 1.0

    def __post_init__(self):
        """Validate that required auto-detected fields are set."""
        required = ['monitor_width_cm', 'monitor_height_cm', 'monitor_width_px',
                    'monitor_height_px', 'display_fps']
        missing = [f for f in required if getattr(self, f) is None]
        if missing:
            raise ValueError(
                f"Missing required monitor properties: {missing}. "
                f"Use StimulusParams.from_monitor() to auto-detect."
            )

    @classmethod
    def from_monitor(cls, monitor_index: int = 1, **overrides) -> 'StimulusParams':
        """
        Create StimulusParams with auto-detected monitor properties.

        Args:
            monitor_index: Monitor to detect (default: 1 = stimulus monitor)
            **overrides: Override any parameter after detection

        Returns:
            StimulusParams with detected monitor values
        """
        info = detect_monitor(monitor_index)
        logging.info(f"Detected monitor {monitor_index}: {info['name']}")
        logging.info(f"  Resolution: {info['width_px']}x{info['height_px']} @ {info['refresh_rate']}Hz")
        logging.info(f"  Physical: {info['width_cm']:.1f}cm x {info['height_cm']:.1f}cm")

        params = cls(
            monitor_width_cm=info['width_cm'],
            monitor_height_cm=info['height_cm'],
            monitor_width_px=info['width_px'],
            monitor_height_px=info['height_px'],
            display_fps=float(info['refresh_rate']),
            **overrides
        )
        return params


def generate_checkerboard_gpu(width_px, height_px, checker_size_px, bar_width_px, bar_position_px, direction='horizontal', device='cuda'):
    """Generate moving bar with checkerboard pattern using GPU."""
    # Create coordinate grids on GPU
    x = torch.arange(width_px, device=device)
    y = torch.arange(height_px, device=device)
    xx, yy = torch.meshgrid(x, y, indexing='xy')

    # Checkerboard pattern
    checker_x = (xx // checker_size_px) % 2
    checker_y = (yy // checker_size_px) % 2
    checkerboard = (checker_x + checker_y) % 2

    # Bar mask
    if direction == 'horizontal':
        bar_mask = (xx >= bar_position_px) & (xx < bar_position_px + bar_width_px)
    else:  # vertical
        bar_mask = (yy >= bar_position_px) & (yy < bar_position_px + bar_width_px)

    # Create frame with black background and checkerboard inside bar
    frame = torch.full((height_px, width_px), 0.0, dtype=torch.float32, device=device)

    # Apply checkerboard pattern only inside bar
    frame = torch.where(bar_mask, checkerboard.float(), frame)

    return frame


def apply_spherical_projection_gpu(frames, width_px, height_px, monitor_width_cm, monitor_height_cm,
                                    distance_cm, angle_horizontal_deg, angle_vertical_deg, device='cuda'):
    """
    Apply spherical projection transform to frames on GPU.
    Based on Marshel et al. (2011) Supplemental Experimental Procedures.
    """
    # Convert angles to radians
    theta_h = np.radians(angle_horizontal_deg)
    theta_v = np.radians(angle_vertical_deg)

    # Create meshgrid for monitor coordinates (centered at 0) on GPU
    x = torch.linspace(-monitor_width_cm/2, monitor_width_cm/2, width_px, device=device)
    y = torch.linspace(-monitor_height_cm/2, monitor_height_cm/2, height_px, device=device)
    X, Y = torch.meshgrid(x, y, indexing='xy')

    # Rotation matrices (numpy for matrix mult, then to GPU)
    R_h = torch.tensor([
        [np.cos(theta_h), 0, np.sin(theta_h)],
        [0, 1, 0],
        [-np.sin(theta_h), 0, np.cos(theta_h)]
    ], dtype=torch.float32, device=device)

    R_v = torch.tensor([
        [1, 0, 0],
        [0, np.cos(theta_v), -np.sin(theta_v)],
        [0, np.sin(theta_v), np.cos(theta_v)]
    ], dtype=torch.float32, device=device)

    # Monitor points in 3D (shape: 3 x num_pixels)
    Z_const = torch.full((height_px, width_px), distance_cm, device=device)
    points = torch.stack([X.flatten(), Y.flatten(), Z_const.flatten()], dim=0)

    # Apply rotations: R_v @ (R_h @ points)
    points_rotated = R_v @ (R_h @ points)

    # Project onto sphere
    r = torch.norm(points_rotated, dim=0, keepdim=True)
    sphere_points = points_rotated / r * distance_cm

    # Map back to monitor plane
    X_proj = sphere_points[0].reshape(height_px, width_px)
    Y_proj = sphere_points[1].reshape(height_px, width_px)

    # Convert to pixel coordinates
    x_indices = ((X_proj + monitor_width_cm/2) / monitor_width_cm * width_px).long()
    y_indices = ((Y_proj + monitor_height_cm/2) / monitor_height_cm * height_px).long()

    # Clip to valid range
    x_indices = torch.clamp(x_indices, 0, width_px - 1)
    y_indices = torch.clamp(y_indices, 0, height_px - 1)

    # Apply transform to all frames (GPU-accelerated)
    num_frames = frames.shape[0]
    transformed = torch.zeros_like(frames)

    for i in range(num_frames):
        transformed[i] = frames[i][y_indices, x_indices]

    return transformed


class StimulusGenerator:
    """
    On-demand stimulus generator for preview and acquisition.

    Generates individual frames without storing entire sequence in memory.
    Pre-computes spherical projection mapping for efficiency.
    """

    def __init__(self, params: StimulusParams, direction: str, use_gpu: bool = True, flip_horizontal: bool = False,
                 width_px: int = None, height_px: int = None):
        """
        Initialize generator with config and direction.

        Args:
            params: StimulusParams with monitor and stimulus settings
            direction: 'LR', 'RL', 'TB', or 'BT'
            use_gpu: Whether to use GPU acceleration
            flip_horizontal: Flip stimulus horizontally
            width_px: Optional explicit width (overrides params calculation)
            height_px: Optional explicit height (overrides params calculation)
        """
        self.params = params
        self.direction = direction
        self.flip_horizontal = flip_horizontal
        self.device = torch.device('cuda' if (use_gpu and torch.cuda.is_available()) else 'cpu')

        # Calculate dimensions - use explicit if provided, else from params
        if width_px and height_px:
            self.width_px = width_px
            self.height_px = height_px
        else:
            self.width_px = int(params.monitor_width_px * params.scale_factor)
            self.height_px = int(params.monitor_height_px * params.scale_factor)

        # Calculate checker size in pixels
        pixels_per_cm = self.width_px / params.monitor_width_cm
        self.pixels_per_deg = pixels_per_cm * params.monitor_distance_cm * np.tan(np.radians(1.0))
        self.checker_size_px = int(params.checker_size_deg * self.pixels_per_deg)
        self.bar_width_px = int(params.stimulus_width_deg * self.pixels_per_deg)

        # Calculate step size (bar moves this many pixels per frame)
        step_size_deg = params.sweep_speed_deg_per_sec / params.display_fps
        self.step_size_px = step_size_deg * self.pixels_per_deg

        # Determine sweep dimension
        if direction in ['LR', 'RL']:
            self.sweep_dimension_px = self.width_px
            self.sweep_direction = 'horizontal'
            self.sweep_dimension_cm = params.monitor_width_cm
        else:  # TB, BT
            self.sweep_dimension_px = self.height_px
            self.sweep_direction = 'vertical'
            self.sweep_dimension_cm = params.monitor_height_cm

        # Calculate total number of frames
        self.num_frames = int(self.sweep_dimension_px / self.step_size_px)

        # Counter-phase strobing: frames per half cycle (use rounding for best approximation)
        self.frames_per_half_cycle = round(params.display_fps / params.strobe_frequency_hz / 2)

        # Calculate actual strobe frequency achieved
        actual_strobe_hz = params.display_fps / (2 * self.frames_per_half_cycle)
        if abs(actual_strobe_hz - params.strobe_frequency_hz) > 0.5:
            logging.warning(f"  Strobe frequency approximation: target {params.strobe_frequency_hz} Hz, actual {actual_strobe_hz:.2f} Hz")

        # Pre-compute spherical projection mapping (ONCE - this is the expensive part!)
        self._compute_projection_mapping()

        logging.info(f"StimulusGenerator initialized for {direction}:")
        logging.info(f"  {self.num_frames} frames, {self.width_px}x{self.height_px} px")
        logging.info(f"  GPU: {self.device.type == 'cuda'}")

    def _compute_projection_mapping(self):
        """
        Pre-compute spherical projection coordinates for all pixels.

        Following Marshel et al. 2011 methodology.
        Computes (azimuth, altitude) in spherical space for each pixel,
        then inverts to find which flat checkerboard pixel maps to each screen pixel.
        """
        # Calculate screen field of view
        screen_width_deg = 2 * np.degrees(np.arctan(self.params.monitor_width_cm / (2 * self.params.monitor_distance_cm)))
        screen_height_deg = 2 * np.degrees(np.arctan(self.params.monitor_height_cm / (2 * self.params.monitor_distance_cm)))

        # Create pixel coordinate grids - use torch if GPU available
        if self.device.type == 'cuda':
            y_px = torch.arange(self.height_px, dtype=torch.float32, device=self.device)
            x_px = torch.arange(self.width_px, dtype=torch.float32, device=self.device)
            Y_px, X_px = torch.meshgrid(y_px, x_px, indexing='ij')

            # Convert pixel coordinates to degrees (centered at screen center)
            X_degrees = (X_px - self.width_px / 2) * (screen_width_deg / self.width_px)
            Y_degrees = (Y_px - self.height_px / 2) * (screen_height_deg / self.height_px)

            # Convert degrees to cm on screen using field of view mapping
            y_screen_cm = X_degrees * (self.params.monitor_width_cm / screen_width_deg)
            z_screen_cm = Y_degrees * (self.params.monitor_height_cm / screen_height_deg)

            # Create 3D Cartesian coordinates (x₀, y, z) per Marshel paper
            x0 = torch.full_like(y_screen_cm, self.params.monitor_distance_cm)
            y = y_screen_cm
            z = z_screen_cm

            # Calculate distance (r)
            r = torch.sqrt(x0**2 + y**2 + z**2)

            # Apply exact Marshel equations from SI page 16
            azimuth_rad = torch.atan2(-y, x0)
            altitude_rad = np.pi/2 - torch.acos(torch.clamp(z / r, -1.0, 1.0))

            # Convert to degrees - keep as tensors on GPU
            self.azimuth_deg = torch.rad2deg(azimuth_rad)
            self.altitude_deg = torch.rad2deg(altitude_rad)

            if self.flip_horizontal:
                self.azimuth_deg = -self.azimuth_deg
                self.altitude_deg = -self.altitude_deg

            self.use_gpu = True
        else:
            # CPU path with numpy
            y_px = np.arange(self.height_px, dtype=np.float32)
            x_px = np.arange(self.width_px, dtype=np.float32)
            Y_px, X_px = np.meshgrid(y_px, x_px, indexing='ij')

            X_degrees = (X_px - self.width_px / 2) * (screen_width_deg / self.width_px)
            Y_degrees = (Y_px - self.height_px / 2) * (screen_height_deg / self.height_px)

            y_screen_cm = X_degrees * (self.params.monitor_width_cm / screen_width_deg)
            z_screen_cm = Y_degrees * (self.params.monitor_height_cm / screen_height_deg)

            x0 = np.full_like(y_screen_cm, self.params.monitor_distance_cm)
            y = y_screen_cm
            z = z_screen_cm

            r = np.sqrt(x0**2 + y**2 + z**2)

            azimuth_rad = np.arctan2(-y, x0)
            altitude_rad = np.pi/2 - np.arccos(np.clip(z / r, -1.0, 1.0))

            self.azimuth_deg = np.degrees(azimuth_rad)
            self.altitude_deg = np.degrees(altitude_rad)

            if self.flip_horizontal:
                self.azimuth_deg = -self.azimuth_deg
                self.altitude_deg = -self.altitude_deg

            self.use_gpu = False

    def get_num_frames(self):
        """Get total number of frames for this direction."""
        return self.num_frames

    def get_angle(self, frame_idx: int) -> float:
        """Get visual angle (degrees) for a given frame index."""
        # Account for direction reversal
        if self.direction in ['RL', 'BT']:
            frame_idx = self.num_frames - 1 - frame_idx

        # Calculate bar position
        bar_position_px = int(frame_idx * self.step_size_px)

        # Calculate visual angle
        position_cm = (bar_position_px / (self.width_px / self.params.monitor_width_cm)) - self.sweep_dimension_cm / 2
        angle_deg = np.degrees(np.arctan2(position_cm, self.params.monitor_distance_cm))

        return angle_deg

    def generate_frame(self, frame_idx: int) -> np.ndarray:
        """
        Generate a single frame with spherical projection.

        Args:
            frame_idx: Frame index (0 to num_frames-1)

        Returns:
            Frame as uint8 numpy array (height, width), range 0-255
        """
        # Account for direction reversal
        if self.direction in ['RL', 'BT']:
            actual_idx = self.num_frames - 1 - frame_idx
        else:
            actual_idx = frame_idx

        # Calculate bar position in DEGREES (not pixels!)
        bar_position_deg = actual_idx * (self.step_size_px / self.pixels_per_deg)

        # Adjust for starting position (bar should start off-screen)
        if self.direction in ['LR', 'RL']:
            # For LR/RL: azimuth sweep
            screen_width_deg = 2 * np.degrees(np.arctan(self.params.monitor_width_cm / (2 * self.params.monitor_distance_cm)))
            start_position = -(screen_width_deg / 2) - self.params.stimulus_width_deg
            bar_angle = start_position + bar_position_deg
        else:
            # For TB/BT: altitude sweep
            screen_height_deg = 2 * np.degrees(np.arctan(self.params.monitor_height_cm / (2 * self.params.monitor_distance_cm)))
            start_position = (screen_height_deg / 2) + self.params.stimulus_width_deg
            bar_angle = start_position - bar_position_deg

        # Determine if checkerboard polarity should be inverted (counter-phase strobing)
        half_cycle = frame_idx // self.frames_per_half_cycle
        invert_polarity = (half_cycle % 2 == 1)

        # Generate checkerboard in SPHERICAL SPACE with counter-phase strobing
        frame = self._generate_checkerboard_spherical(bar_angle, invert_polarity)

        # Convert to 0-255 uint8 and ensure contiguous array for OpenGL
        frame_uint8 = (frame * 255).astype(np.uint8)
        frame_contiguous = np.ascontiguousarray(frame_uint8)

        return frame_contiguous

    def _generate_checkerboard_spherical(self, bar_angle: float, invert_polarity: bool = False) -> np.ndarray:
        """
        Generate checkerboard pattern in SPHERICAL SPACE.

        This is the correct approach - the pattern is generated directly in
        spherical coordinates (azimuth, altitude), not screen pixels.

        Args:
            bar_angle: Bar position in degrees (azimuth for LR/RL, altitude for TB/BT)
            invert_polarity: If True, invert checkerboard polarity (counter-phase strobing)

        Returns:
            Frame as float32 numpy array (height, width), range 0-1
        """
        if self.use_gpu:
            # GPU path with PyTorch
            frame = torch.zeros((self.height_px, self.width_px), dtype=torch.float32, device=self.device)

            # Determine bar region in spherical coordinates
            if self.direction in ['LR', 'RL']:
                bar_center = -bar_angle
                bar_mask = (self.azimuth_deg >= bar_center - self.params.stimulus_width_deg/2) & \
                          (self.azimuth_deg < bar_center + self.params.stimulus_width_deg/2)
            else:  # TB, BT
                bar_center = bar_angle
                bar_mask = (self.altitude_deg >= bar_center - self.params.stimulus_width_deg/2) & \
                          (self.altitude_deg < bar_center + self.params.stimulus_width_deg/2)

            # Create checkerboard pattern in SPHERICAL SPACE
            azimuth_checks = torch.floor(self.azimuth_deg / self.params.checker_size_deg).to(torch.int64)
            altitude_checks = torch.floor(self.altitude_deg / self.params.checker_size_deg).to(torch.int64)
            checkerboard = ((azimuth_checks + altitude_checks) % 2).to(torch.float32)

            # Apply counter-phase strobing
            if invert_polarity:
                checkerboard = 1.0 - checkerboard

            # Apply checkerboard to bar region
            frame[bar_mask] = checkerboard[bar_mask]

            # Transfer back to CPU as numpy
            return frame.cpu().numpy()
        else:
            # CPU path with numpy
            frame = np.full((self.height_px, self.width_px), 0.0, dtype=np.float32)

            if self.direction in ['LR', 'RL']:
                bar_center = -bar_angle
                bar_mask = (self.azimuth_deg >= bar_center - self.params.stimulus_width_deg/2) & \
                          (self.azimuth_deg < bar_center + self.params.stimulus_width_deg/2)
            else:  # TB, BT
                bar_center = bar_angle
                bar_mask = (self.altitude_deg >= bar_center - self.params.stimulus_width_deg/2) & \
                          (self.altitude_deg < bar_center + self.params.stimulus_width_deg/2)

            azimuth_checks = np.floor(self.azimuth_deg / self.params.checker_size_deg).astype(np.int64)
            altitude_checks = np.floor(self.altitude_deg / self.params.checker_size_deg).astype(np.int64)
            checkerboard = ((azimuth_checks + altitude_checks) % 2).astype(np.float32)

            if invert_polarity:
                checkerboard = 1.0 - checkerboard

            frame[bar_mask] = checkerboard[bar_mask]

            return frame


def generate_direction(params: StimulusParams, direction: str, output_dir: Path, flip_horizontal: bool = False,
                       width_px: int = None, height_px: int = None):
    """
    Generate all frames for a single direction and save to HDF5.

    Uses StimulusGenerator class to ensure consistency with acquisition.

    Args:
        params: Stimulus parameters
        direction: One of 'LR', 'RL', 'TB', 'BT'
        output_dir: Directory to save output
        flip_horizontal: Whether to flip stimulus horizontally (for rear-projection)
        width_px: Optional explicit width (overrides params calculation)
        height_px: Optional explicit height (overrides params calculation)

    Returns:
        Tuple of (num_frames, min_angle, max_angle)
    """
    # Use the same StimulusGenerator class that acquisition uses
    # This ensures the generated library matches what was previewed
    generator = StimulusGenerator(params, direction, use_gpu=torch.cuda.is_available(), flip_horizontal=flip_horizontal,
                                  width_px=width_px, height_px=height_px)
    num_frames = generator.get_num_frames()

    logging.info(f"Direction {direction}:")
    logging.info(f"  Dimensions: {generator.width_px}x{generator.height_px} px")
    logging.info(f"  Total frames: {num_frames}")
    logging.info(f"  Generating (GPU: {torch.cuda.is_available()})...")

    # Generate all frames
    all_frames = []
    angles = np.zeros(num_frames, dtype=np.float32)

    for frame_idx in range(num_frames):
        frame = generator.generate_frame(frame_idx)
        angles[frame_idx] = generator.get_angle(frame_idx)
        all_frames.append(frame)

        # Progress update every 100 frames
        if (frame_idx + 1) % 100 == 0:
            logging.info(f"    {frame_idx + 1}/{num_frames} frames...")

    all_frames = np.array(all_frames)

    logging.info(f"  Frame range: {all_frames.min()} to {all_frames.max()}")
    logging.info(f"  Angle range: {angles.min():.2f}° to {angles.max():.2f}°")

    # Save to HDF5
    output_file = output_dir / f"{direction}_frames.h5"
    with h5py.File(output_file, 'w') as f:
        f.create_dataset('frames', data=all_frames, compression='gzip', compression_opts=4)
        f.create_dataset('angles', data=angles)

    logging.info(f"  Saved to {output_file}")

    return len(all_frames), angles.min(), angles.max()


def main():
    """Generate stimulus library."""

    # Load config
    config_path = Path("C:/Program Files/Kim-Neuroscience-Lab/KimLabISI/isi_config.json")
    with open(config_path, 'r') as f:
        config = json.load(f)

    logging.info("=" * 80)
    logging.info("STIMULUS GENERATION")
    logging.info("=" * 80)

    # Get monitor index and settings from config
    monitor_index = config['display']['index']
    fps_divisor = config['display']['target_fps_divisor']
    flip_horizontal = config['display'].get('inverted', False)
    scale_factor = config['display'].get('scale_factor', 1.0)

    # Check for explicit resolution override
    explicit_width = config['display'].get('resolution_width')
    explicit_height = config['display'].get('resolution_height')

    logging.info("")
    logging.info("=== MONITOR DETECTION ===")

    # Auto-detect monitor properties and create params
    params = StimulusParams.from_monitor(
        monitor_index=monitor_index,
        monitor_distance_cm=config['display']['distance_cm'],
        angle_horizontal_deg=config['display']['angle_horizontal_deg'],
        angle_vertical_deg=config['display']['angle_vertical_deg'],
        stimulus_width_deg=config['stimulus']['stimulus_width_deg'],
        checker_size_deg=config['stimulus']['checker_size_deg'],
        sweep_speed_deg_per_sec=config['stimulus']['sweep_speed_deg_per_sec'],
        strobe_frequency_hz=config['stimulus']['strobe_frequency_hz'],
        scale_factor=scale_factor
    )

    # Store native refresh rate before applying divisor
    native_refresh_rate = params.display_fps

    # Apply FPS divisor
    params.display_fps = params.display_fps / fps_divisor

    logging.info("")
    logging.info("=== DISPLAY CONFIG ===")
    logging.info(f"Monitor index: {monitor_index}")
    logging.info(f"Native refresh rate: {native_refresh_rate:.0f} Hz")
    logging.info(f"FPS divisor: {fps_divisor}")
    logging.info(f"Effective FPS: {params.display_fps:.1f} Hz")
    logging.info(f"Scale factor: {scale_factor}")
    logging.info(f"Horizontal flip: {flip_horizontal}")
    logging.info(f"Monitor distance: {config['display']['distance_cm']} cm")

    logging.info("")
    logging.info("=== STIMULUS CONFIG ===")
    logging.info(f"Stimulus width: {config['stimulus']['stimulus_width_deg']}°")
    logging.info(f"Checker size: {config['stimulus']['checker_size_deg']}°")
    logging.info(f"Sweep speed: {config['stimulus']['sweep_speed_deg_per_sec']}°/s")
    logging.info(f"Strobe frequency: {config['stimulus']['strobe_frequency_hz']} Hz")

    # Calculate output resolution
    # Use explicit resolution if configured, else auto-detect with scale_factor
    if explicit_width and explicit_height:
        width_px = explicit_width
        height_px = explicit_height
        logging.info(f"Using configured resolution: {width_px}x{height_px}")
    else:
        width_px = int(params.monitor_width_px * scale_factor)
        height_px = int(params.monitor_height_px * scale_factor)
        logging.info(f"Using monitor resolution: {width_px}x{height_px}")

    # Calculate pixels per degree for visual angle conversions
    pixels_per_cm = width_px / params.monitor_width_cm
    pixels_per_deg = pixels_per_cm * params.monitor_distance_cm * np.tan(np.radians(1.0))

    step_size_deg = params.sweep_speed_deg_per_sec / params.display_fps
    step_size_px = step_size_deg * pixels_per_deg

    # Calculate FOV
    fov_horizontal = 2 * np.degrees(np.arctan(params.monitor_width_cm / (2 * params.monitor_distance_cm)))
    fov_vertical = 2 * np.degrees(np.arctan(params.monitor_height_cm / (2 * params.monitor_distance_cm)))

    logging.info("")
    logging.info("=== CALCULATED VALUES ===")
    logging.info(f"Output resolution: {width_px}x{height_px} px")
    logging.info(f"Pixels per degree: {pixels_per_deg:.2f}")
    logging.info(f"Field of view: {fov_horizontal:.1f}° x {fov_vertical:.1f}°")
    logging.info(f"Step size: {step_size_deg:.3f}°/frame = {step_size_px:.2f} px/frame")

    # Frame counts
    frames_horizontal = int(width_px / step_size_px)
    frames_vertical = int(height_px / step_size_px)
    duration_horizontal = frames_horizontal / params.display_fps
    duration_vertical = frames_vertical / params.display_fps

    logging.info(f"Horizontal sweep: {frames_horizontal} frames ({duration_horizontal:.2f}s)")
    logging.info(f"Vertical sweep: {frames_vertical} frames ({duration_vertical:.2f}s)")

    # Strobe timing
    frames_per_half_cycle = round(params.display_fps / params.strobe_frequency_hz / 2)
    actual_strobe_hz = params.display_fps / (2 * frames_per_half_cycle)
    logging.info(f"Strobe: {frames_per_half_cycle} frames/half-cycle = {actual_strobe_hz:.2f} Hz actual")

    # Output directory
    output_dir = Path("C:/Program Files/Kim-Neuroscience-Lab/KimLabISI/stimulus_library")
    output_dir.mkdir(exist_ok=True)

    logging.info("")
    logging.info("=== GENERATION ===")
    logging.info(f"Output directory: {output_dir}")
    logging.info(f"GPU available: {torch.cuda.is_available()}")
    if torch.cuda.is_available():
        logging.info(f"GPU device: {torch.cuda.get_device_name(0)}")
    logging.info("")

    # Generate all directions
    metadata = {
        'generation_params': asdict(params),
        'directions': config['acquisition']['directions'],
        'total_frames': 0
    }

    for direction in config['acquisition']['directions']:
        num_frames, angle_min, angle_max = generate_direction(
            params, direction, output_dir,
            flip_horizontal=flip_horizontal,
            width_px=width_px, height_px=height_px
        )
        metadata['total_frames'] += num_frames
        logging.info("")

    # Save metadata
    metadata_file = output_dir / 'library_metadata.json'
    with open(metadata_file, 'w') as f:
        json.dump(metadata, f, indent=2)

    logging.info("=" * 80)
    logging.info(f"GENERATION COMPLETE")
    logging.info(f"Total frames: {metadata['total_frames']}")
    logging.info(f"Metadata saved to {metadata_file}")
    logging.info("=" * 80)


if __name__ == "__main__":
    main()
