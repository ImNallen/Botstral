#!/usr/bin/env python3
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shlex
import sqlite3
import stat
import subprocess
import sys
import uuid


def private_file(path):
    descriptor = os.open(path, os.O_RDWR | os.O_NOFOLLOW)
    metadata = os.fstat(descriptor)
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.geteuid() or stat.S_IMODE(metadata.st_mode) != 0o600 or metadata.st_size > 65536:
        os.close(descriptor)
        raise ValueError('private regular file required')
    return os.fdopen(descriptor, 'r+')


def reconcile(request):
    root = Path(request['root'])
    identity = str(uuid.UUID(request['run']))
    if not root.is_absolute() or root.name != identity:
        raise ValueError('fixture identity mismatch')
    ledger_path = Path(request['ledger'])
    if not ledger_path.is_absolute():
        raise ValueError('absolute campaign ledger required')
    with private_file(root / 'state/manager.lock') as manager_lock:
        fcntl.flock(manager_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        database = root / 'state/manager.sqlite3'
        if database.is_symlink() or not database.is_file():
            raise ValueError('fixture database required')
        connection = sqlite3.connect('file:' + str(database) + '?mode=ro', uri=True)
        attempts = []
        try:
            for table in ('run_turns', 'coding_calls', 'inferences'):
                for (encoded,) in connection.execute('SELECT view FROM ' + table):
                    value = json.loads(encoded)
                    cost = value['reservation']['cost_microusd']
                    if type(cost) is not int or cost < 0:
                        raise ValueError('invalid durable reservation')
                    record = value.get('id', value.get('request', {}).get('id'))
                    attempt = value.get('attempt', value['phase'].get('attempt') if isinstance(value['phase'], dict) else None)
                    attempts.append({'kind':table, 'record':record, 'attempt':attempt,
                                     'retained_microusd':cost, 'phase':value['phase']})
                    if len(attempts) > 4096:
                        raise ValueError('attempt retention bound exceeded')
        finally:
            connection.close()
        retained = sum(a['retained_microusd'] for a in attempts)
        hashes = {p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in
                  (database, database.with_name(database.name + '-wal')) if p.exists()}
        with private_file(ledger_path.with_name(ledger_path.name + '.lock')) as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            with private_file(ledger_path) as stream:
                ledger = json.load(stream)
            reservation = next(r for r in ledger['reservations'] if r['run'] == identity)
            previous = reservation['microusd']
            if retained > previous:
                raise ValueError('durable attempts exceed the campaign reservation')
            if retained == previous:
                return ledger
            reservation['microusd'] = retained
            ledger.setdefault('reconciliations', []).append({
                'run':identity, 'previous_microusd':previous, 'retained_microusd':retained,
                'attempts':attempts, 'state_sha256':hashes, 'evidence_sha256':request['evidence_sha256']})
            temporary = ledger_path.with_name(ledger_path.name + '.' + str(uuid.uuid4()) + '.tmp')
            with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), 'w') as output:
                json.dump(ledger, output)
                output.flush()
                os.fsync(output.fileno())
            os.replace(temporary, ledger_path)
            directory = os.open(ledger_path.parent, os.O_RDONLY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
            return ledger


def main():
    parser = argparse.ArgumentParser(description='Retain every durable attempt ceiling and release only unused acceptance allocations after fixture shutdown.')
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--ledger')
    parser.add_argument('--manager-ssh', default='botstral-manager')
    parser.add_argument('--remote', action='store_true')
    args = parser.parse_args()
    os.umask(0o077)
    if args.remote:
        print(json.dumps(reconcile(json.load(sys.stdin))))
        return
    if not args.evidence or not args.ledger:
        parser.error('--evidence and --ledger are required')
    data = args.evidence.read_bytes()
    report = json.loads(data)
    shutdown = [c for c in report['checks'] if '.shutdown.' in c['name'] or c['name'] == 'process_cleanup']
    if not any(c['name'] == 'process_cleanup' for c in shutdown) or not any('.shutdown.' in c['name'] for c in shutdown) or any(c['status'] != 'passed' for c in shutdown):
        raise ValueError('successful fixture shutdown and cleanup evidence required')
    request = {'root':report['root'], 'run':Path(report['root']).name,
               'ledger':args.ledger, 'evidence_sha256':hashlib.sha256(data).hexdigest()}
    source = Path(__file__).read_text()
    result = subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', '--',
        args.manager_ssh, shlex.join(['python3', '-c', source, '--remote'])],
        input=json.dumps(request), text=True, capture_output=True, check=True)
    print(result.stdout.strip())


if __name__ == '__main__':
    main()
