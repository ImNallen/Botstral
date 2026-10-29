#!/usr/bin/env python3
"""Run recorded-provider memory checks and retain their actual process results."""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--full', action='store_true', help='Also run both repository verification scripts.')
    parser.add_argument('--playwright-module', type=Path, help='Ephemeral Playwright index.mjs for component video.')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    evidence = root / 'docs/evidence/conversational-memory'
    evidence.mkdir(parents=True, exist_ok=True)
    logs = evidence / 'logs'
    logs.mkdir(exist_ok=True)
    environment = os.environ.copy()
    environment['BOTSTRAL_MEMORY_EVIDENCE'] = str(evidence)
    environment['BOTSTRAL_MEMORY_NATIVE_VIEWS'] = str(evidence / 'native-conversations.json')
    commands = [
        ('memory', ['cargo', 'test', '-p', 'botstral-manager', '--locked', '--features', 'nonproduction-test-worker', '--lib', 'memory_tests', '--', '--nocapture']),
        ('connector-secrets', ['cargo', 'test', '-p', 'botstral-manager', '--locked', '--lib', 'conversational_memory_excludes_connector', '--', '--nocapture']),
        ('native-memory', ['python3', 'desktop/fixtures/paired_manager.py', '--verify-memory']),
        ('fixture-types', ['npx', 'tsc', '-p', 'desktop/fixtures/tsconfig.memory.json']),
        ('frontend-tests', ['npm', 'test']),
    ]
    if args.full:
        commands.extend([
            ('repository', ['./scripts/verify.sh']),
            ('desktop', ['./scripts/verify-desktop.sh']),
        ])
    if args.playwright_module:
        commands.append(('component-video', ['node', 'desktop/fixtures/conversational-memory.mjs', str(args.playwright_module.resolve())]))
    results = []
    for name, command in commands:
        started = datetime.now(timezone.utc).isoformat()
        with (logs / f'{name}.log').open('w') as output:
            completed = subprocess.run(command, cwd=root, env=environment, stdout=output, stderr=subprocess.STDOUT, check=False)
        log = logs / f'{name}.log'
        content = log.read_bytes().rstrip(b'\n')
        log.write_bytes(content + (b'\n' if content else b''))
        result = {'name': name, 'command': command, 'started': started, 'finished': datetime.now(timezone.utc).isoformat(), 'exit_code': completed.returncode, 'log': f'logs/{name}.log'}
        results.append(result)
        (evidence / 'commands.json').write_text(json.dumps(results, indent=2) + '\n')
        print(f'{name}: exit {completed.returncode}', flush=True)
        if completed.returncode:
            raise SystemExit(completed.returncode)


if __name__ == '__main__':
    main()
