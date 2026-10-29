#!/usr/bin/env python3
import http.client
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import ssl
import stat
import shutil
import sys

import m06_remote


def dispatch(root, request):
    operation = request['op']
    if operation in ('guest_read', 'guest_write', 'guest_cleanup'):
        workspace = Path(json.loads((root / 'guest-workspace.json').read_text()))
        assert workspace == root / 'workspace' or (workspace.parent == Path('/home/botstral') and workspace.name.startswith('acceptance-'))
        if operation == 'guest_cleanup':
            for path in (workspace, workspace.with_name(workspace.name + '-retired')):
                if path.exists():
                    assert not path.is_symlink() and path.stat().st_uid == os.getuid()
                    shutil.rmtree(path)
            return True
        assert workspace.is_dir() and not workspace.is_symlink() and workspace.stat().st_uid == os.getuid()
        if operation == 'guest_read':
            assert Path(request['name']).name == request['name'] and request['name'] not in ('.', '..')
            path = workspace / request['name']
            return path.read_text() if path.exists() else None
        for name, value in request['files'].items():
            assert Path(name).name == name and name not in ('.', '..')
            path = workspace / name
            if path.exists():
                assert path.read_text() == value
                continue
            with path.open('x') as stream:
                stream.write(value)
        return True
    if operation == 'verify_sources':
        source = (root / 'src').resolve()
        changed = []
        for name, expected in request['manifest'].items():
            path = source / name
            assert path.resolve().is_relative_to(source)
            if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
                changed.append(name)
        return {'files': len(request['manifest']), 'changed': changed}
    if operation == 'reserve':
        sys.path.insert(0, str(root / 'src/agent/tests'))
        from m04_remote import acceptance_budget
        return acceptance_budget(request)
    if operation == 'reconcile_budget':
        sys.path.insert(0, str(root / 'src/agent/tests'))
        from m04_reconcile_budget import reconcile
        return reconcile(request)
    if operation == 'provision':
        path = root / 'provider-key'
        metadata = path.lstat()
        assert stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.getuid()
        assert stat.S_IMODE(metadata.st_mode) == 0o600 and 0 < metadata.st_size <= 8192
        with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW)) as stream:
            key = stream.read(8193).strip()
        response = m06_remote.dispatch(root, dict(op='http', address=request['address'],
            method='PUT', path='/v1/provider-keys/mistral', token=request['token'], body={'secret': key}))
        assert response['status'] == 200
        return {'provisioned': True}
    if operation == 'observe':
        with sqlite3.connect('file:' + str(root / 'state/manager.sqlite3') + '?mode=ro', uri=True) as database:
            return {name: [json.loads(row[0]) for row in database.execute(query)] for name, query in {
                'occurrences': 'SELECT view FROM routine_occurrences ORDER BY scheduled_at,id',
                'runs': 'SELECT summary FROM runs ORDER BY rowid',
                'tasks': 'SELECT view FROM tasks ORDER BY rowid',
                'inbox': 'SELECT view FROM routine_inbox ORDER BY sequence',
            }.items()}
    if operation == 'worker_http':
        config = json.loads((root / 'agent/config.json').read_text())
        context = ssl.create_default_context(cafile=config['manager_ca'])
        context.load_cert_chain(config['client_cert'], config['client_key'])
        address = config['manager'].removeprefix('wss://')
        host, port = address.rsplit(':', 1)
        connection = http.client.HTTPSConnection(host, int(port), context=context, timeout=10)
        try:
            connection.request(request['method'], request['path'], json.dumps(request.get('body', {})), {'Content-Type': 'application/json'})
            response = connection.getresponse()
            data = response.read()
            return {'status': response.status, 'body': data[:4096].decode(errors='replace')}
        finally:
            connection.close()
    if operation == 'scan_provider':
        if not (root / 'provider-key').exists():
            return {'files': 0, 'leaks': None, 'verdict': 'NOT SCANNED', 'reason': 'comparison key unavailable'}
        key = (root / 'provider-key').read_bytes().strip()
        leaks = []
        files = 0
        for directory in ('state', 'public-evidence'):
            for path in (root / directory).rglob('*'):
                if path.is_file():
                    files += 1
                    if key in path.read_bytes():
                        leaks.append(str(path.relative_to(root)))
        for path in root.glob('*.log'):
            files += 1
            if key in path.read_bytes():
                leaks.append(path.name)
        return {'files': files, 'leaks': leaks, 'verdict': 'SCANNED'}
    if operation == 'provider_requests':
        with sqlite3.connect('file:' + str(root / 'state/manager.sqlite3') + '?mode=ro', uri=True) as database:
            return [{'request': json.loads(request), 'response': json.loads(view),
                'endpoint': json.loads(configuration)['policy']['endpoint']}
                for request, view, configuration in database.execute(
                    'SELECT t.provider_request,t.view,r.configuration FROM run_turns t JOIN runs r ON r.id=t.run ORDER BY r.rowid,t.ordinal')]
    return m06_remote.dispatch(root, request)


if __name__ == '__main__':
    os.umask(0o077)
    try:
        result = dispatch(Path(__file__).resolve().parent, json.load(sys.stdin))
        print(json.dumps({'ok': True, 'value': result}))
    except Exception as error:
        print(json.dumps({'ok': False, 'error': type(error).__name__}))
        sys.exit(1)
