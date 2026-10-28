#!/usr/bin/env python3
"""Run offline manager recovery without starting the service."""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from recovery import main
sys.exit(main('manager'))
