#!/usr/bin/env python3
"""Write or verify the C02 component source manifest."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--write', action='store_true')
parser.add_argument('--check-controls', action='store_true')
parser.add_argument('--manifest', type=Path, default=ROOT/'docs/evidence/c02/source.json')
args = parser.parse_args()
paths = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
prefixes = ('agent/', 'manager/', 'link/', 'desktop/', 'scripts/', '.github/')
shared = {'Cargo.toml', 'Cargo.lock', 'Makefile', 'package.json', 'package-lock.json', 'rust-toolchain.toml'}
source = sorted(path for path in paths if path and
                (path.startswith(prefixes) or path in shared) and '/tests/evidence/' not in path)
actual = {path: hashlib.sha256((ROOT/path).read_bytes()).hexdigest() for path in source}
if args.write:
    record = {
        'recorded_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'source_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'scope': 'Tracked component files, shared build files, scripts and workflows, excluding historical test evidence.',
        'sha256': actual,
    }
    args.manifest.write_text(json.dumps(record, indent=2)+'\n')
else:
    record = json.loads(args.manifest.read_text())
    expected = record['sha256']
    changed = sorted(path for path in actual.keys() | expected.keys() if actual.get(path) != expected.get(path))
    if changed:
        print('C02 source manifest mismatch: '+', '.join(changed), file=sys.stderr)
        sys.exit(1)
print(f'C02 source manifest matches {len(actual)} files.')

if args.check_controls:
    with tempfile.TemporaryDirectory(prefix='c02-source-controls-') as temporary:
        for control in ['changed_hash', 'missing_file', 'extra_file']:
            candidate = json.loads(json.dumps(record))
            first = next(iter(candidate['sha256']))
            if control == 'changed_hash':
                candidate['sha256'][first] = '0'*64
            elif control == 'missing_file':
                del candidate['sha256'][first]
            else:
                candidate['sha256']['desktop/missing-source.rs'] = '0'*64
            path = Path(temporary)/(control+'.json')
            path.write_text(json.dumps(candidate))
            result = subprocess.run([sys.executable, __file__, '--manifest', str(path)], capture_output=True, text=True)
            if result.returncode != 1 or 'C02 source manifest mismatch:' not in result.stderr:
                raise RuntimeError('Source control did not reject '+control)
            print('PASS rejects '+control)
