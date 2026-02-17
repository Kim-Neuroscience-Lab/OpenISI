# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for OpenISI Camera Daemon.

Bundles Python runtime with all dependencies for standalone execution.
Platform-specific dependencies (pyobjc on macOS) are handled automatically.

Build command:
    cd daemon && poetry run pyinstaller openisi-daemon.spec --distpath ../dist
"""

import os
import sys
from PyInstaller.utils.hooks import collect_submodules, collect_data_files

block_cipher = None

# Collect all daemon submodules
daemon_hiddenimports = collect_submodules('daemon')

# Platform-specific imports
platform_hiddenimports = []
if sys.platform == 'darwin':
    # macOS: AVFoundation camera backend
    platform_hiddenimports.extend([
        'AVFoundation',
        'CoreMedia',
        'Quartz',
        'objc',
        'Foundation',
        'libdispatch',
    ])
    platform_hiddenimports.extend(collect_submodules('AVFoundation'))
    platform_hiddenimports.extend(collect_submodules('CoreMedia'))
    platform_hiddenimports.extend(collect_submodules('Quartz'))
elif sys.platform in ('win32', 'linux'):
    # Windows/Linux: PCO camera SDK
    platform_hiddenimports.extend(collect_submodules('pco'))

# All hidden imports
all_hiddenimports = [
    'numpy',
    'cv2',
    'serial',
    'serial.tools',
    'serial.tools.list_ports',
    *daemon_hiddenimports,
    *platform_hiddenimports,
]

a = Analysis(
    ['entry.py'],
    pathex=[os.path.dirname(os.path.abspath(SPECPATH))],  # Add parent dir for daemon package
    binaries=[],
    datas=[],
    hiddenimports=all_hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        # Exclude unnecessary packages to reduce size
        'tkinter',
        'matplotlib',
        'PIL',
        'scipy',
        'pandas',
    ],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name='openisi-daemon',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=True,  # Daemon runs headless, needs console for logging
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name='openisi-daemon',
)
