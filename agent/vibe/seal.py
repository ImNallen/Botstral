#!/usr/bin/env python3
"""Record an installed Vibe runtime without modifying it."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def seal(root, destination):
    distributions = json.loads(subprocess.run([str(root / 'bin/python3'), '-B', '-c',
        'import importlib.metadata,json; print(json.dumps({d.metadata["Name"].lower():d.version for d in importlib.metadata.distributions()}))'],
        check=True, capture_output=True, text=True).stdout)
    if distributions.get('mistral-vibe') != '2.24.2':
        raise ValueError('Vibe 2.24.2 is required')
    files = []
    for path in sorted(root.rglob('*')):
        if path.is_file() and '__pycache__' not in path.parts and path.suffix != '.pyc':
            with path.open('rb') as source:
                files.append([str(path.relative_to(root)), hashlib.file_digest(source, 'sha256').hexdigest()])
    value = {'version': '2.24.2', 'distributions': distributions, 'files': files}
    with destination.open('x') as output:
        json.dump(value, output, sort_keys=True, separators=(',', ':'))
    print(hashlib.sha256(destination.read_bytes()).hexdigest())


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--runtime', required=True, type=Path)
    parser.add_argument('--manifest', required=True, type=Path)
    args = parser.parse_args()
    seal(args.runtime.resolve(), args.manifest)
