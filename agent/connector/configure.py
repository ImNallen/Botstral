#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--python', type=Path, required=True)
parser.add_argument('--uid', type=int, default=os.getuid())
parser.add_argument('--gid', type=int, default=os.getgid())
args = parser.parse_args()
if args.uid <= 0 or args.gid < 0:
    parser.error('a non-root guest account is required')
python = args.python.resolve(strict=True)
python_digest = hashlib.sha256(python.read_bytes()).hexdigest()
runtime = hashlib.sha256(Path(__file__).with_name('runtime.py').read_bytes() + python_digest.encode()).hexdigest()
print(json.dumps({'python': str(python), 'python_sha256': python_digest, 'runtime': runtime,
                  'account': {'uid': args.uid, 'gid': args.gid},
                  'ceilings': {'runtime_seconds': 60, 'output_bytes': 8192, 'processes': 32, 'memory_bytes': 536870912}}, indent=2))
