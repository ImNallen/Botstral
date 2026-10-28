#!/usr/bin/env python3
"""Run a real manager with a recorded provider and an explicitly synthetic worker."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'manager/tests'))
import m05_acceptance as m


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path)
    parser.add_argument('--duration', type=int, default=3600)
    parser.add_argument('--verify-native', action='store_true', help='Run the actual-manager contract with the real OS keychain, then clean up.')
    args = parser.parse_args()
    os.umask(0o077)
    temporary = tempfile.TemporaryDirectory(prefix='botstral-c02-native-') if args.root is None else None
    root = Path(temporary.name) if temporary else args.root.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    if (root/'fixture.json').exists():
        raise RuntimeError('Choose a fresh fixture directory.')
    m.certificates(root)
    env = dict(os.environ, BOTSTRAL_C02_FIXTURE_ROOT=str(root))
    with (root/'manager.log').open('w') as log:
        process = subprocess.Popen(['cargo', 'test', '-p', 'botstral-manager', '--locked', '--lib', 'c02_desktop_fixture', '--', '--ignored', '--nocapture'], cwd=ROOT, env=env, stdout=log, stderr=log, start_new_session=True)
        try:
            until = time.monotonic()+180
            while not (root/'fixture.json').exists():
                if process.poll() is not None or time.monotonic() > until:
                    raise RuntimeError('Fixture startup failed. Inspect '+str(root/'manager.log'))
                time.sleep(.1)
            fixture = json.loads((root/'fixture.json').read_text())
            print('Fixture root: '+str(root), flush=True)
            print('Local manager: '+fixture['local'], flush=True)
            print('Synthetic password: '+fixture['password'], flush=True)
            print('Trusted export: '+str(root/'trust.json'), flush=True)
            print('Browser runtime digest: '+fixture['runtime'], flush=True)
            print('Worker is synthetic. No real process or browser execution is proved.', flush=True)
            print('Use the packaged app. Open local administration, sign in, return to the paired workspace, create a request, authorize with local manager, choose trust.json, and confirm the identity and pin.', flush=True)
            print('Isolated debug profile: '+str(root/'desktop-profile'), flush=True)
            print('Launch the debug app executable with BOTSTRAL_DESKTOP_PROFILE set to that path. Sign out the paired device before stopping this fixture.', flush=True)
            if args.verify_native:
                result = subprocess.run(['cargo', 'test', '--manifest-path', 'desktop/src-tauri/Cargo.toml', '--no-default-features', '--locked', '--test', 'remote', 'actual_manager_native', '--', '--ignored', '--nocapture'], cwd=ROOT, env=env)
                if result.returncode:
                    raise RuntimeError('The native contract failed.')
                return
            until = time.monotonic()+args.duration
            while process.poll() is None and time.monotonic() < until and not (root/'stop').exists():
                time.sleep(.2)
        except KeyboardInterrupt:
            pass
        finally:
            (root/'stop').touch()
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=10)
            if temporary:
                temporary.cleanup()

if __name__ == '__main__':
    main()
