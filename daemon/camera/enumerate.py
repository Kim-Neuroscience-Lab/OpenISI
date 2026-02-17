"""Camera enumeration utilities."""

import json
import subprocess
import sys

import cv2


def _get_device_names_macos() -> dict[int, str]:
    """Get camera device names on macOS using system_profiler."""
    names = {}
    try:
        result = subprocess.run(
            ["system_profiler", "SPCameraDataType", "-json"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            cameras = data.get("SPCameraDataType", [])
            for i, cam in enumerate(cameras):
                name = cam.get("_name", f"Camera {i}")
                names[i] = name
    except Exception:
        pass
    return names


def _get_device_names_linux() -> dict[int, str]:
    """Get camera device names on Linux using v4l2."""
    names = {}
    try:
        import os
        for device in sorted(os.listdir("/dev")):
            if device.startswith("video"):
                try:
                    idx = int(device[5:])
                    device_path = f"/dev/{device}"
                    result = subprocess.run(
                        ["v4l2-ctl", "-d", device_path, "--info"],
                        capture_output=True,
                        text=True,
                        timeout=2,
                    )
                    if result.returncode == 0:
                        for line in result.stdout.split("\n"):
                            if "Card type" in line:
                                name = line.split(":", 1)[1].strip()
                                names[idx] = name
                                break
                except (ValueError, subprocess.TimeoutExpired):
                    pass
    except Exception:
        pass
    return names


def _get_device_names_windows() -> dict[int, str]:
    """Get camera device names on Windows using DirectShow via ffmpeg."""
    names = {}
    try:
        result = subprocess.run(
            ["ffmpeg", "-list_devices", "true", "-f", "dshow", "-i", "dummy"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        output = result.stderr
        idx = 0
        for line in output.split("\n"):
            if "(video)" in line.lower():
                if '"' in line:
                    name = line.split('"')[1]
                    names[idx] = name
                    idx += 1
    except Exception:
        pass
    return names


def _get_device_names() -> dict[int, str]:
    """Get camera device names for the current platform."""
    if sys.platform == "darwin":
        return _get_device_names_macos()
    elif sys.platform == "linux":
        return _get_device_names_linux()
    elif sys.platform == "win32":
        return _get_device_names_windows()
    return {}


def enumerate_opencv_cameras(max_devices: int = 10) -> list[dict]:
    """Enumerate cameras accessible via OpenCV.

    Args:
        max_devices: Maximum number of device indices to check.

    Returns:
        List of dicts with device info from hardware detection.
    """
    device_names = _get_device_names()
    devices = []

    for i in range(max_devices):
        cap = cv2.VideoCapture(i)
        if cap.isOpened():
            # Capture a test frame to get actual format from hardware
            ret, frame = cap.read()
            if not ret or frame is None:
                cap.release()
                continue

            # Get actual frame properties from the captured frame
            height, width = frame.shape[:2]
            num_channels = frame.shape[2] if len(frame.shape) > 2 else 1
            bits_per_component = frame.dtype.itemsize * 8
            bits_per_pixel = bits_per_component * num_channels

            fps = cap.get(cv2.CAP_PROP_FPS)
            name = device_names.get(i, f"Camera {i}")

            devices.append({
                "index": i,
                "name": name,
                "width": width,
                "height": height,
                "fps": fps,
                "bits_per_pixel": bits_per_pixel,
                "bits_per_component": bits_per_component,
                "num_channels": num_channels,
                "dtype": str(frame.dtype),
            })
            cap.release()

    return devices


def enumerate_all_cameras() -> dict:
    """Enumerate all available cameras across all backends.

    Returns:
        Dict with camera backend info. On macOS, uses AVFoundation (hardware timestamps).
        On other platforms, uses OpenCV.
        {
            "avfoundation": {"available": bool, "devices": [...]},  # macOS only
            "opencv": {"available": bool, "devices": [...]},        # non-macOS
            "pco": {"available": bool, "devices": []},              # scientific cameras
        }
    """
    result = {}

    # On macOS, use AVFoundation for hardware timestamps
    if sys.platform == "darwin":
        try:
            from .avfoundation import is_available, enumerate_cameras
            if is_available():
                avf_cameras = enumerate_cameras()
                if avf_cameras:
                    result["avfoundation"] = {
                        "available": True,
                        "devices": avf_cameras,
                    }
        except ImportError as e:
            print(f"AVFoundation import failed: {e}", file=sys.stderr)

    # OpenCV as fallback on non-macOS or if AVFoundation unavailable
    if "avfoundation" not in result:
        opencv_cameras = enumerate_opencv_cameras()
        if opencv_cameras:
            result["opencv"] = {
                "available": True,
                "devices": opencv_cameras,
            }

    # Check PCO cameras (scientific cameras on Windows/Linux)
    try:
        from .pco import is_available, enumerate_cameras
        if is_available():
            pco_cameras = enumerate_cameras()
            if pco_cameras:
                result["pco"] = {
                    "available": True,
                    "devices": pco_cameras,
                }
    except ImportError:
        pass

    return result


def main():
    """CLI entry point - outputs JSON to stdout."""
    cameras = enumerate_all_cameras()
    print(json.dumps(cameras, indent=2))


if __name__ == "__main__":
    main()
