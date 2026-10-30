#!/usr/bin/env python3
"""Verify export fencing across two owned local manager fixtures."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / 'docs/evidence/p05'


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    processes = []
    records = []
    result = None
    with tempfile.TemporaryDirectory(prefix='botstral-p05-managers-') as temporary:
        roots = [Path(temporary) / name for name in ['manager-a', 'manager-b']]
        try:
            for index, fixture in enumerate(roots):
                command = [sys.executable, 'desktop/fixtures/paired_manager.py', '--root', str(fixture), '--duration', '240']
                log = (EVIDENCE / f'manager-switch-fixture-{index}.log').open('w')
                process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
                processes.append((process, log, fixture))
            deadline = time.monotonic() + 180
            while not all((fixture / 'fixture.json').exists() for fixture in roots):
                if any(process.poll() is not None for process, _, _ in processes) or time.monotonic() > deadline:
                    raise RuntimeError('Owned P05 manager fixture startup failed.')
                time.sleep(.1)
            env = dict(os.environ, BOTSTRAL_C02_FIXTURE_ROOT=str(roots[0]), BOTSTRAL_P05_OTHER_MANAGER=str(roots[1]))
            command = ['cargo', 'test', '--manifest-path', 'desktop/src-tauri/Cargo.toml', '--no-default-features', '--locked', '--test', 'journal', 'p05_export_manager_switch', '--', '--ignored', '--exact', '--nocapture']
            with (EVIDENCE / 'manager-switch.log').open('w') as log:
                result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
                log.write(f'\nEXIT_CODE={result.returncode}\n')
            records.append({'command': command, 'exit_code': result.returncode})
        finally:
            for process, log, fixture in processes:
                fixture.mkdir(exist_ok=True)
                (fixture / 'stop').touch()
                try:
                    code = process.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    code = process.wait(timeout=10)
                log.close()
                records.append({'fixture': str(fixture), 'exit_code': code})
            (EVIDENCE / 'manager-switch.json').write_text(json.dumps(records, indent=2) + '\n')
    return result.returncode if result else 1


if __name__ == '__main__':
    sys.exit(main())
