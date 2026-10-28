#!/usr/bin/env python3
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'manager/packaging'))
from install import main
sys.exit(main('agent', Path(__file__).resolve().parent))
