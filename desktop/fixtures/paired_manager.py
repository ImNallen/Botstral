#!/usr/bin/env python3
"""Run a real manager with a recorded provider and an explicitly synthetic worker."""
import argparse
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'manager/tests'))
import m05_acceptance as m


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path)
    parser.add_argument('--duration', type=int, default=3600)
    verification = parser.add_mutually_exclusive_group()
    verification.add_argument('--verify-native', action='store_true', help='Run the actual-manager contract with the real OS keychain, then clean up.')
    verification.add_argument('--verify-memory', action='store_true', help='Verify conversational memory, obsolete journals, and manager restart.')
    verification.add_argument('--verify-journal', action='store_true', help='Run actual-manager journal recovery contracts without an OS keychain.')
    parser.add_argument('--journal-case', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.journal_case == 'conversational_memory_and_obsolete_journal':
        args.verify_memory = True
    if args.verify_memory:
        args.verify_journal = True
        args.journal_case = 'conversational_memory_and_obsolete_journal'
    if args.journal_case and not args.verify_journal:
        parser.error('--journal-case requires --verify-journal')
    if args.verify_journal and args.journal_case is None:
        discovery = subprocess.run(['cargo', 'test', '--manifest-path', 'desktop/src-tauri/Cargo.toml', '--no-default-features', '--locked', '--test', 'journal', '--', '--list', '--ignored'], cwd=ROOT, capture_output=True, text=True)
        if discovery.returncode:
            raise RuntimeError('Journal contract discovery failed.\n'+discovery.stderr)
        cases = [line[:-6] for line in discovery.stdout.splitlines() if line.endswith(': test')]
        if not cases:
            raise RuntimeError('No journal contracts were discovered.')
        for case in cases:
            command = [sys.executable, __file__, '--verify-journal', '--journal-case', case]
            if args.root:
                command += ['--root', str(args.root/case)]
            subprocess.run(command, cwd=ROOT, check=True)
        return
    if args.verify_journal and args.journal_case == 'p06_native_receipt_manager_binding':
        subprocess.run([sys.executable, str(ROOT / 'desktop/fixtures/p06-manager-switch.py')], cwd=ROOT, check=True)
        return
    if args.verify_journal and args.journal_case == 'p05_export_manager_switch':
        subprocess.run([sys.executable, str(ROOT / 'desktop/fixtures/p05-manager-switch.py')], cwd=ROOT, check=True)
        return
    os.umask(0o077)
    temporary = tempfile.TemporaryDirectory(prefix='botstral-c02-native-') if args.root is None else None
    root = Path(temporary.name) if temporary else args.root.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    if (root/'fixture.json').exists():
        raise RuntimeError('Choose a fresh fixture directory.')
    m.certificates(root)
    env = dict(os.environ, BOTSTRAL_C02_FIXTURE_ROOT=str(root), BOTSTRAL_C02_VIEW_OUTPUT=str(root/'desktop-view.json'))
    if args.journal_case == 'p05_saved_results_restart_and_export':
        env['BOTSTRAL_P05_PROOF'] = '1'
    if args.verify_memory or args.journal_case in ('p05_saved_results_restart_and_export', 'p05_attachments_restart_and_export'):
        env['BOTSTRAL_MEMORY_PROOF'] = '1'
    with (root/'manager.log').open('w') as log:
        process = subprocess.Popen(['cargo', 'test', '-p', 'botstral-manager', '--locked', '--lib', 'c02_desktop_fixture', '--', '--ignored', '--nocapture'], cwd=ROOT, env=env, stdout=log, stderr=log, start_new_session=True)
        try:
            waiting = time.monotonic()
            until = waiting+180
            while not (root/'fixture.json').exists():
                if process.poll() is not None or time.monotonic() > until:
                    raise RuntimeError('Fixture startup failed. Inspect '+str(root/'manager.log'))
                time.sleep(.1)
            fixture = json.loads((root/'fixture.json').read_text())
            trust = json.loads((root/'trust.json').read_text())
            print('Fixture root: '+str(root), flush=True)
            print('Local manager: '+fixture['local'], flush=True)
            print('Paired WSS route: wss://'+fixture['public']+'/owner/v1/connect', flush=True)
            print('Manager identity: '+trust['manager'], flush=True)
            print('Manager leaf: '+trust['leaf'], flush=True)
            if not os.environ.get('BOTSTRAL_P06_EVIDENCE'):
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
                boundary = subprocess.run(['npm', 'test', '--', 'src/paired-contract.test.ts'], cwd=ROOT, env=env)
                if boundary.returncode:
                    raise RuntimeError('The native renderer boundary contract failed.')
                return
            if args.verify_journal:
                if os.environ.get('BOTSTRAL_SIGNIN_TIMING'):
                    print('SIGNIN_TIMING '+json.dumps({'role':'harness','phase':'fixture_wait','ms':int((time.monotonic()-waiting)*1000)}), flush=True)
                journal = subprocess.run(['cargo', 'test', '--manifest-path', 'desktop/src-tauri/Cargo.toml', '--no-default-features', '--locked', '--test', 'journal', args.journal_case, '--', '--ignored', '--exact', '--nocapture'], cwd=ROOT, env=env)
                if journal.returncode:
                    raise RuntimeError('The native journal contract failed.')
                if os.environ.get('BOTSTRAL_SIGNIN_TIMING'):
                    for index in range(5):
                        payload = json.dumps({'password': fixture['password']}).encode()
                        request = urllib.request.Request('http://'+fixture['local']+'/v1/sessions', data=payload, headers={'content-type':'application/json'}, method='POST')
                        probe = time.monotonic()
                        with urllib.request.urlopen(request, timeout=15) as response:
                            status = response.status
                            response.read()
                        print('SIGNIN_TIMING '+json.dumps({'role':'probe','phase':'grant','n':index,'ms':int((time.monotonic()-probe)*1000),'status':status}), flush=True)
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
            if os.environ.get('BOTSTRAL_SIGNIN_TIMING'):
                try:
                    for line in (root/'manager.log').read_text().splitlines():
                        if 'SIGNIN_TIMING' in line:
                            print(line, flush=True)
                except FileNotFoundError:
                    pass
            if args.verify_memory and os.environ.get('BOTSTRAL_MEMORY_NATIVE_VIEWS'):
                shutil.copyfile(root/'manager.log', Path(os.environ['BOTSTRAL_MEMORY_NATIVE_VIEWS']).with_name('native-manager.log'))
            if temporary:
                temporary.cleanup()

if __name__ == '__main__':
    main()
