#!/usr/bin/env python3
"""Run P06 acceptance checks and preserve each invocation's exact exit status."""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', action='store_true')
    parser.add_argument('--full', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    evidence = root / 'docs/evidence/p06' / stamp
    evidence.mkdir(parents=True)
    environment = {**os.environ, 'BOTSTRAL_P06_EVIDENCE': str(evidence)}
    commands = ([('handoff', ['cargo', 'test', '-p', 'botstral-manager', '--locked', '--lib',
                              'limits_and_cancellation_precede_publication_and_vault_handoff_has_no_value'])]
                if args.baseline else
                [('credentials', ['cargo', 'test', '-p', 'botstral-manager', '--locked', '--lib', 'credentials::'])])
    if not args.baseline:
        commands.append(('native-storage', ['python3', 'desktop/fixtures/paired_manager.py', '--verify-journal', '--journal-case', 'p06_protected_entry_never_enters_native_journal']))
    if args.full:
        commands += [('repository', ['./scripts/verify.sh']), ('desktop', ['./scripts/verify-desktop.sh'])]
    results = []
    for name, command in commands:
        started = datetime.now(timezone.utc).isoformat()
        log = evidence / f'{name}.log'
        with log.open('w') as output:
            result = subprocess.run(command, cwd=root, env=environment, stdout=output,
                                    stderr=subprocess.STDOUT, check=False)
        results.append(dict(name=name, command=command, started=started,
                            finished=datetime.now(timezone.utc).isoformat(),
                            exit_code=result.returncode, log=log.name))
        (evidence / 'commands.json').write_text(json.dumps(results, indent=2) + '\n')
        print(f'{name}: exit {result.returncode}; {log}', flush=True)
        if result.returncode:
            raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
