"""Entry point for PyInstaller (`python -m mimic_engine`)."""

import sys

from mimic_engine.cli import main

if __name__ == "__main__":
    sys.exit(main())
