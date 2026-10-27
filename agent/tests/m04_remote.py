#!/usr/bin/env python3
"""Two-VM acceptance helpers. Provider key handling runs on Manager only."""
import fcntl
import json
import os
os.umask(0o077)
from pathlib import Path
import stat
import subprocess
import sys
import uuid
import w01_computer_remote as computer
import m02_remote


def acceptance_budget(r):
    path = Path(r['path'])
    if not path.is_absolute() or str(uuid.UUID(r['run'])) != r['run']:
        raise ValueError('absolute ledger and canonical run identity required')
    lock_path = path.with_name(path.name + '.lock')
    with os.fdopen(os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600), 'r+') as lock:
        if stat.S_IMODE(os.fstat(lock.fileno()).st_mode) != 0o600:
            raise ValueError('private budget lock required')
        fcntl.flock(lock, fcntl.LOCK_EX)
        if os.path.lexists(path):
            meta = path.lstat()
            if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid() or stat.S_IMODE(meta.st_mode) != 0o600 or meta.st_size > 65536:
                raise ValueError('private budget ledger required')
            ledger = json.loads(path.read_text())
        else:
            ledger = {'cap_microusd':r['cap_microusd'], 'previous_reserved_microusd':r['previous_reserved_microusd'], 'reservations':[]}
        if ledger['cap_microusd'] != r['cap_microusd'] or ledger['previous_reserved_microusd'] != r['previous_reserved_microusd'] or not 0 <= r['previous_reserved_microusd'] <= r['cap_microusd'] <= 1000000:
            raise ValueError('acceptance cap cannot change')
        reservation = {'run':r['run'], 'microusd':r['reservation_microusd']}
        existing = next((v for v in ledger['reservations'] if v['run'] == r['run']), None)
        if existing:
            if existing != reservation:
                raise ValueError('acceptance reservation changed')
            return ledger
        if not 0 < reservation['microusd'] or ledger['previous_reserved_microusd'] + sum(v['microusd'] for v in ledger['reservations']) + reservation['microusd'] > ledger['cap_microusd']:
            raise ValueError('acceptance cap exhausted')
        ledger['reservations'].append(reservation)
        temporary = path.with_name(path.name + '.' + r['run'] + '.tmp')
        with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), 'w') as output:
            json.dump(ledger, output)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
        return ledger


def dispatch(r):
    if r['op'] == 'acceptance_budget':
        return acceptance_budget(r)
    if r['op'] == 'provision_provider':
        path = Path(r['provider_key_file'])
        meta = path.lstat()
        if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid() or stat.S_IMODE(meta.st_mode) != 0o600 or not 0 < meta.st_size <= 8192:
            raise ValueError('provider key file must be a private bounded regular file')
        with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW)) as stream:
            key = stream.read(8193).strip()
        result = m02_remote.http_request({'address':r['address'], 'method':'PUT', 'path':'/v1/provider-keys/mistral',
                                          'token':r['token'], 'body':{'secret':key}})
        if result['status'] != 200:
            raise ValueError('provider provisioning failed')
        return {'provisioned':True}
    if r['op'] == 'seal_vibe':
        result = subprocess.run(['/usr/bin/python3', '-B', r['script'], '--runtime', r['runtime'], '--manifest', r['manifest']],
                                capture_output=True, text=True, timeout=90, check=True)
        return {'sha256':result.stdout.strip()}
    if r['op'] == 'mkdir':
        Path(r['path']).mkdir(mode=0o700)
        return True
    return computer.dispatch(r)


if __name__ == '__main__':
    os.umask(0o077)
    try:
        print(json.dumps({'ok':True, 'value':dispatch(json.load(sys.stdin))}))
    except Exception as error:
        print(json.dumps({'ok':False, 'error':type(error).__name__}))
        sys.exit(1)
