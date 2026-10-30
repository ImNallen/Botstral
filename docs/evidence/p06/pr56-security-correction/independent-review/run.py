#!/usr/bin/env python3
"""Re-run the independent PR56 probes against the recorded production hashes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import tempfile


def main():
    evidence = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=evidence.parents[4])
    args = parser.parse_args()
    repo = args.repo.resolve()
    manifest = json.loads((evidence / 'review-source.json').read_text())
    for relative, expected in manifest.items():
        actual = hashlib.sha256((repo / relative).read_bytes()).hexdigest()
        if actual != expected:
            raise SystemExit(f'Source mismatch: {relative}')
    with tempfile.TemporaryDirectory(prefix='p06-independent-') as temporary:
        scratch = Path(temporary)
        files = subprocess.check_output(['git', 'ls-files', '-z'], cwd=repo).split(b'\0')
        for encoded in files:
            if not encoded:
                continue
            relative = Path(os.fsdecode(encoded))
            if relative.parts[0] in {'docs', '.github', '.agents', '.claude'}:
                continue
            source = repo / relative
            if not source.is_file():
                continue
            destination = scratch / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
        probes = scratch / 'independent-probes.rs'
        shutil.copyfile(evidence / 'probes.rs', probes)
        registration = scratch / 'manager/src/manager/state/credentials/tests/live.rs'
        with registration.open('a') as output:
            output.write('\n#[path = ' + json.dumps(str(probes)) + '] mod independent_probes;\n')
        environment = {**os.environ, 'CARGO_TARGET_DIR': str(repo / 'target')}
        subprocess.run(['cargo', 'test', '-p', 'botstral-manager', '--lib', '--locked',
                        'independent_review_', '--', '--nocapture'], cwd=scratch,
                       env=environment, check=True)
        recovery = (evidence / 'recovery_probe.py').read_text()
        recovery = recovery.replace(
            "source = Path('/Users/nallen/.t3/worktrees/Botstral/t3-010ce7cb')",
            'source = Path(' + repr(str(repo)) + ')', 1)
        script = scratch / 'recovery_probe.py'
        script.write_text(recovery)
        runpy.run_path(str(script), run_name='__main__')


if __name__ == '__main__':
    main()
