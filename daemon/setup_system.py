#!/usr/bin/env python3
"""Cross-platform system dependency setup for OpenISI.

Handles installation of system-level dependencies that can't be managed by Poetry:
- Vulkan/MoltenVK for hardware vsync timestamps
- Rust toolchain for GDExtension compilation

Usage:
    poetry run python -m daemon.setup_system
    poetry run python -m daemon.setup_system --check  # Check only, don't install
"""

import subprocess
import sys
import shutil
from pathlib import Path


def get_platform() -> str:
    """Get current platform identifier."""
    if sys.platform == "darwin":
        return "macos"
    elif sys.platform == "linux":
        return "linux"
    elif sys.platform == "win32":
        return "windows"
    else:
        return "unknown"


def run_cmd(cmd: list[str], check: bool = True) -> subprocess.CompletedProcess:
    """Run a command and return result."""
    print(f"  Running: {' '.join(cmd)}")
    return subprocess.run(cmd, capture_output=True, text=True, check=check)


def check_vulkan() -> bool:
    """Check if Vulkan is available."""
    platform = get_platform()

    if platform == "macos":
        # Need BOTH libvulkan.dylib (loader) AND MoltenVK (ICD)
        # ash loads libvulkan.dylib which then loads MoltenVK
        loader_paths = [
            "/opt/homebrew/lib/libvulkan.dylib",
            "/usr/local/lib/libvulkan.dylib",
        ]
        icd_paths = [
            "/opt/homebrew/lib/libMoltenVK.dylib",
            "/usr/local/lib/libMoltenVK.dylib",
        ]

        has_loader = any(Path(p).exists() for p in loader_paths)
        has_icd = any(Path(p).exists() for p in icd_paths)

        if has_loader and has_icd:
            print("  Found Vulkan loader and MoltenVK ICD")
            return True
        elif has_icd and not has_loader:
            print("  Found MoltenVK but missing vulkan-loader")
            return False
        return False

    elif platform == "linux":
        # Check for libvulkan
        result = run_cmd(["ldconfig", "-p"], check=False)
        if "libvulkan" in result.stdout:
            print("  Found Vulkan via ldconfig")
            return True
        # Also check common paths
        paths = ["/usr/lib/libvulkan.so", "/usr/lib/x86_64-linux-gnu/libvulkan.so"]
        for path in paths:
            if Path(path).exists():
                print(f"  Found Vulkan: {path}")
                return True
        return False

    elif platform == "windows":
        # Vulkan typically comes with GPU drivers on Windows
        vulkan_dll = Path("C:/Windows/System32/vulkan-1.dll")
        if vulkan_dll.exists():
            print(f"  Found Vulkan: {vulkan_dll}")
            return True
        return False

    return False


def check_rust() -> bool:
    """Check if Rust toolchain is available."""
    if shutil.which("cargo"):
        result = run_cmd(["cargo", "--version"], check=False)
        if result.returncode == 0:
            print(f"  Found Rust: {result.stdout.strip()}")
            return True
    return False


def install_vulkan() -> bool:
    """Install Vulkan/MoltenVK for current platform."""
    platform = get_platform()

    if platform == "macos":
        if not shutil.which("brew"):
            print("  ERROR: Homebrew not found. Install from https://brew.sh")
            return False

        # Install both MoltenVK and vulkan-loader
        # vulkan-loader provides libvulkan.dylib which loads MoltenVK
        print("  Installing MoltenVK and Vulkan loader via Homebrew...")
        result = run_cmd(["brew", "install", "molten-vk", "vulkan-loader"], check=False)
        if result.returncode != 0:
            print(f"  ERROR: {result.stderr}")
            return False
        return True

    elif platform == "linux":
        # Try to detect package manager
        if shutil.which("apt-get"):
            print("  Installing Vulkan via apt...")
            result = run_cmd(
                ["sudo", "apt-get", "install", "-y", "libvulkan1", "libvulkan-dev"],
                check=False
            )
            return result.returncode == 0

        elif shutil.which("dnf"):
            print("  Installing Vulkan via dnf...")
            result = run_cmd(
                ["sudo", "dnf", "install", "-y", "vulkan-loader", "vulkan-loader-devel"],
                check=False
            )
            return result.returncode == 0

        elif shutil.which("pacman"):
            print("  Installing Vulkan via pacman...")
            result = run_cmd(
                ["sudo", "pacman", "-S", "--noconfirm", "vulkan-icd-loader"],
                check=False
            )
            return result.returncode == 0

        else:
            print("  ERROR: No supported package manager found (apt, dnf, pacman)")
            return False

    elif platform == "windows":
        print("  Vulkan on Windows comes with GPU drivers.")
        print("  Please update your GPU drivers if Vulkan is not available.")
        print("  Or install Vulkan SDK from: https://vulkan.lunarg.com/sdk/home")
        return False

    return False


def install_rust() -> bool:
    """Install Rust toolchain."""
    if shutil.which("rustup"):
        print("  Rust already managed by rustup")
        return True

    print("  Installing Rust via rustup...")
    platform = get_platform()

    if platform in ("macos", "linux"):
        result = run_cmd(
            ["sh", "-c", "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"],
            check=False
        )
        return result.returncode == 0

    elif platform == "windows":
        print("  Please install Rust from: https://rustup.rs")
        return False

    return False


def get_library_path() -> str:
    """Get the library path for Vulkan on this platform."""
    platform = get_platform()
    if platform == "macos":
        # Homebrew paths
        for path in ["/opt/homebrew/lib", "/usr/local/lib"]:
            if Path(path).exists():
                return path
    return ""


def main():
    """Main entry point."""
    import argparse
    parser = argparse.ArgumentParser(description="Setup OpenISI system dependencies")
    parser.add_argument("--check", action="store_true", help="Check only, don't install")
    parser.add_argument("--env", action="store_true", help="Print environment setup commands")
    args = parser.parse_args()

    if args.env:
        # Print environment setup for shell
        platform = get_platform()
        lib_path = get_library_path()
        if platform == "macos" and lib_path:
            print(f"export DYLD_LIBRARY_PATH={lib_path}:$DYLD_LIBRARY_PATH")
        elif platform == "linux" and lib_path:
            print(f"export LD_LIBRARY_PATH={lib_path}:$LD_LIBRARY_PATH")
        return 0

    platform = get_platform()
    print(f"OpenISI System Setup ({platform})")
    print("=" * 40)

    all_ok = True

    # Check/install Vulkan
    print("\n[1/2] Vulkan/MoltenVK (hardware vsync timestamps)")
    if check_vulkan():
        print("  Status: OK")
    elif args.check:
        print("  Status: MISSING")
        all_ok = False
    else:
        print("  Status: MISSING - Installing...")
        if install_vulkan():
            print("  Status: INSTALLED")
        else:
            print("  Status: FAILED")
            all_ok = False

    # Check/install Rust
    print("\n[2/2] Rust toolchain (GDExtension compilation)")
    if check_rust():
        print("  Status: OK")
    elif args.check:
        print("  Status: MISSING")
        all_ok = False
    else:
        print("  Status: MISSING - Installing...")
        if install_rust():
            print("  Status: INSTALLED")
        else:
            print("  Status: FAILED")
            all_ok = False

    # Summary
    print("\n" + "=" * 40)
    if all_ok:
        print("All system dependencies OK!")
        return 0
    else:
        print("Some dependencies missing or failed to install.")
        print("See output above for details.")
        return 1


if __name__ == "__main__":
    sys.exit(main())
