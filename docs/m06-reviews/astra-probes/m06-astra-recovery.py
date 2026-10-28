import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import sys
import subprocess
import tempfile
from unittest.mock import patch
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'manager/packaging'))
import recovery
import diagnose

os.umask(0o077)
with tempfile.TemporaryDirectory(dir=ROOT / '.audit') as temporary:
    root = Path(temporary).resolve()
    state = root / 'state'
    state.mkdir(mode=0o700)
    (state / 'agent.lock').touch(mode=0o600)
    with sqlite3.connect(state / 'agent.sqlite3') as db:
        db.executescript('PRAGMA user_version=2; CREATE TABLE metadata(incarnation TEXT); CREATE TABLE executions(record TEXT);')
        db.execute('INSERT INTO metadata VALUES(?)', (str(uuid.uuid4()),))
    certificate = state / 'identity.pem'
    certificate.write_text('synthetic certificate bytes')
    (state / 'config.json').write_text(json.dumps(dict.fromkeys(('manager_ca','client_cert','client_key'), str(certificate))))
    python = root / 'runtime' / 'python3'
    python.parent.mkdir(mode=0o700)
    python.write_bytes(b'original synthetic interpreter')
    python.chmod(0o700)
    pin = recovery.digest(python)
    import hashlib
    config = {'python': str(python), 'python_sha256': pin,
              'runtime': hashlib.sha256((ROOT/'agent/connector/runtime.py').read_bytes() + pin.encode()).hexdigest(),
              'account': {'uid': os.geteuid(), 'gid': os.getegid()},
              'ceilings': {'runtime_seconds':60,'output_bytes':1024,'processes':32,'memory_bytes':268435456}}
    (state / 'connector.json').write_text(json.dumps(config))
    print('Connector Python included in references:', str(python) in recovery.references(state))
    def worker_probe():
        result = subprocess.run(['cargo','test','-p','botstral-agent','--lib','astra_connector_configuration_probe','--','--nocapture','--ignored'], cwd=ROOT, env=dict(os.environ, M06_ASTRA_STATE=str(state)), text=True, capture_output=True, check=True)
        for line in (result.stdout + result.stderr).splitlines():
            if line.startswith('Production worker configuration'):
                print(line)
    worker_probe()
    python.write_bytes(b'changed synthetic interpreter')
    print('Configured interpreter digest matches:', recovery.digest(python) == config['python_sha256'])
    print('Production runtime diagnostics returned:', diagnose.runtime_hashes(state))
    worker_probe()
    bundle = root / 'backup'
    with patch.object(recovery, 'worker_quiet'):
        recovery.create('agent', state, bundle, {'roots': []})
    print('Backup with no interpreter root verifies:', recovery.verify('agent', bundle).component == 'agent')
    state.rename(root / 'retired-state')
    python.rename(python.with_name('retired-python3'))
    recovery.restore('agent', bundle, state, retired=True)
    restored = recovery.read_json(state / 'connector.json')
    print('Restored configured interpreter exists:', Path(restored['python']).exists())
    print('Restored runtime diagnostics returned:', diagnose.runtime_hashes(state))
    worker_probe()

