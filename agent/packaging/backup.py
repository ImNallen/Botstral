#!/usr/bin/env python3
import sys
from pathlib import Path
location = Path(__file__).resolve().parent
sys.path.insert(0, str(location if (location / 'recovery.py').exists() else location.parents[1] / 'manager/packaging'))
from recovery import main
sys.exit(main('agent'))
