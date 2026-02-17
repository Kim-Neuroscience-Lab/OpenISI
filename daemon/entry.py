#!/usr/bin/env python3
"""Entry point for PyInstaller-bundled daemon.

This script sets up the package context so relative imports work correctly.
"""

import sys
import os

# Add daemon package to path
daemon_dir = os.path.dirname(os.path.abspath(__file__))
parent_dir = os.path.dirname(daemon_dir)
if parent_dir not in sys.path:
    sys.path.insert(0, parent_dir)

# Now import and run main
from daemon.main import main

if __name__ == '__main__':
    sys.exit(main())
