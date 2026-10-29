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
import uuid

import m06_remote


def dispatch(root, request):
    operation = request['op']
    if operation == 'manager_process':
        import subprocess
        unit = request['unit']
        assert unit.startswith('botstral-m06-') and '/' not in unit
        pid = int(subprocess.check_output(['systemctl', '--user', 'show', unit, '--property=MainPID', '--value'], text=True).strip())
        assert pid > 0
        process = Path('/proc') / str(pid)
        executable = (process / 'exe').resolve(strict=True)
        assert executable.is_relative_to(root / 'bin')
        return {'unit': unit, 'pid': pid, 'executable': str(executable),
                'sha256': hashlib.sha256((process / 'exe').read_bytes()).hexdigest(),
                'arguments': (process / 'cmdline').read_bytes().decode().rstrip('\0').split('\0'),
                'recorded_configuration_present': (root / 'recorded.json').exists()}
    if operation == 'compaction_runtime':
        import subprocess
        runtime = root / 'compaction-runtime'
        shutil.copytree(Path('/home/botstral/m04-vibe-2.24.2'), runtime, symlinks=False,
                        ignore=shutil.ignore_patterns('__pycache__', '*.pyc'))
        driver = (root / 'compact.py').read_bytes()
        launcher = runtime / 'bin/vibe'
        launcher.write_bytes(('#!' + str(runtime / 'bin/python3') + '\n').encode() + driver)
        launcher.chmod(0o700)
        result = subprocess.run(['python3', str(root / 'src/agent/vibe/seal.py'), '--runtime', str(runtime),
            '--manifest', str(root / 'compaction-runtime.json')], check=True, capture_output=True, text=True, timeout=90)
        manifest = json.loads((root / 'compaction-runtime.json').read_text())
        files = dict(manifest['files'])
        assert all(files['lib/python3.12/site-packages/vibe/' + name] == digest
                   for name, digest in request['source_digests'].items())
        return {'runtime': result.stdout.strip(), 'directory': str(runtime), 'manifest': str(root / 'compaction-runtime.json'),
                'content_sha256': hashlib.sha256(json.dumps({name: digest for name, digest in files.items()
                    if name != 'bin/vibe'}, sort_keys=True).encode()).hexdigest(),
                'driver_sha256': hashlib.sha256(launcher.read_bytes()).hexdigest()}
    if operation in ('compaction_native', 'compaction_release'):
        workspace = str(uuid.UUID(request['workspace']))
        home = root / 'agent/coding' / workspace
        if operation == 'compaction_release':
            (home / 'compaction-release').touch(exist_ok=False)
            return True
        path = home / 'compaction-evidence.json'
        if not path.exists():
            return None
        assert path.is_file() and not path.is_symlink() and path.stat().st_size <= 4194304
        try:
            return json.loads(path.read_text())
        except json.JSONDecodeError:
            return None
    if operation == 'coding_evidence':
        return coding_evidence(root, request['task'])
    if operation == 'attempt_inventory':
        with sqlite3.connect('file:' + str(root / 'state/manager.sqlite3') + '?mode=ro', uri=True) as database:
            return {table: [json.loads(row[0]) for row in database.execute('SELECT view FROM ' + table + ' ORDER BY rowid')]
                    for table in ('run_turns', 'coding_calls', 'inferences')}
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


CODING_QUERY = '''SELECT c.id,c.task,c.execution,c.generation,c.request,c.configuration,c.view,c.response,
    t.view,l.generation,l.closed,r.summary,r.configuration
    FROM coding_calls c JOIN tasks t ON t.id=c.task
    JOIN coding_leases l ON l.task=c.task AND l.execution=c.execution AND l.generation=c.generation
    LEFT JOIN run_children o ON o.task=c.task LEFT JOIN runs r ON r.id=o.run
    WHERE c.task=? ORDER BY c.rowid'''


def coding_evidence(root, task):
    assert str(uuid.UUID(task)) == task
    with sqlite3.connect('file:' + str(root / 'state/manager.sqlite3') + '?mode=ro', uri=True) as database:
        rows = database.execute(CODING_QUERY, (task,)).fetchall()
        assert len(rows) <= 16
        attempts = []
        for identity, task, execution, generation, request, config, view, response, task_view, lease_generation, closed, parent, parent_config in rows:
            attempts.append({'id': identity, 'task': task, 'execution': execution, 'generation': generation,
                'request': json.loads(request), 'configuration': json.loads(config), 'view': json.loads(view),
                'response': json.loads(response) if response else None, 'task_view': json.loads(task_view),
                'lease': {'generation': lease_generation, 'closed': bool(closed)},
                'parent': json.loads(parent) if parent else None,
                'parent_configuration': json.loads(parent_config) if parent_config else None})
    return {'database': 'state/manager.sqlite3', 'query': CODING_QUERY,
            'reader_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'attempts': attempts}


if __name__ == '__main__':
    os.umask(0o077)
    try:
        result = dispatch(Path(__file__).resolve().parent, json.load(sys.stdin))
        print(json.dumps({'ok': True, 'value': result}))
    except Exception as error:
        print(json.dumps({'ok': False, 'error': type(error).__name__}))
        sys.exit(1)
