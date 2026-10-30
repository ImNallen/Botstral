#!/usr/bin/env python3
"""Verify retained generated results with isolated synthetic manager fixtures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / 'docs/evidence/p05'


def identity():
    paths = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', 'link', 'manager', 'desktop', 'scripts', 'package.json', 'package-lock.json', 'Cargo.toml', 'Cargo.lock'], cwd=ROOT, text=True).splitlines()
    return {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in sorted(set(paths)) if (ROOT / name).is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--full', action='store_true')
    parser.add_argument('--video', action='store_true')
    args = parser.parse_args()
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    before = identity()
    metadata = {
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'source_sha256': before,
        'commands': [],
    }
    commands = [
        ('catalog-bounds', ['cargo', 'test', '-p', 'botstral-link', '--locked', 'catalog_tests']),
        ('catalog-restore', ['cargo', 'test', '-p', 'botstral-manager', '--locked', '--lib', 'offline_restore_reopens']),
        ('catalog-authority', ['cargo', 'test', '-p', 'botstral-manager', '--locked', '--lib', 'enrollment_binding_atomicity']),
        ('native-unit', ['cargo', 'test', '--manifest-path', 'desktop/src-tauri/Cargo.toml', '--locked', '--lib']),
        ('revocation-after', [sys.executable, 'desktop/fixtures/paired_manager.py', '--verify-journal', '--journal-case', 'p05_chooser_rejects_acknowledged_same_generation_revocation']),
        ('chooser-reauthorization', [sys.executable, 'desktop/fixtures/paired_manager.py', '--verify-journal', '--journal-case', 'p05_export_reauthenticates_after_chooser']),
        ('manager-switch-runner', [sys.executable, 'desktop/fixtures/p05-manager-switch.py']),
        ('native-acceptance', [sys.executable, 'desktop/fixtures/paired_manager.py', '--verify-journal', '--journal-case', 'p05_saved_results_restart_and_export']),
        ('desktop-tests', ['npm', 'test']),
    ]
    if args.video:
        commands += [
            ('component-types', ['npx', 'tsc', '--project', 'desktop/fixtures/tsconfig.p05.json']),
            ('component-video', ['node', 'desktop/fixtures/p05-proof.mjs', 'agent/runtime/node_modules/playwright/index.mjs']),
        ]
    if args.full:
        commands += [('verify', ['./scripts/verify.sh']), ('verify-desktop', ['./scripts/verify-desktop.sh'])]
    env = dict(os.environ)
    env.pop('BOTSTRAL_P05_NATIVE_VIEWS', None)
    for name, command in commands:
        target = EVIDENCE / f'{name}.log'
        print(f'Running {name}. Log: {target}', flush=True)
        with target.open('w') as log:
            command_env = dict(env)
            if name == 'native-acceptance':
                command_env['BOTSTRAL_P05_NATIVE_VIEWS'] = str(EVIDENCE / 'native-views.json')
            result = subprocess.run(command, cwd=ROOT, env=command_env, stdout=log, stderr=subprocess.STDOUT)
            log.write(f'\nEXIT_CODE={result.returncode}\n')
        metadata['commands'].append({'command': command, 'exit_code': result.returncode, 'log': str(target.relative_to(ROOT))})
        metadata['source_unchanged'] = identity() == before
        (EVIDENCE / 'source-identity.json').write_text(json.dumps(metadata, indent=2) + '\n')
        if result.returncode:
            raise SystemExit(result.returncode)
    if not metadata['source_unchanged']:
        raise SystemExit('Source changed during verification.')


if __name__ == '__main__':
    main()
