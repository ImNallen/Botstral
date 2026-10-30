import importlib.util
import os
from pathlib import Path
import sqlite3
import sys
import tempfile

source = Path('/Users/nallen/.t3/worktrees/Botstral/t3-010ce7cb')
spec = importlib.util.spec_from_file_location('pr56_recovery', source / 'manager/packaging/recovery.py')
recovery = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = recovery
spec.loader.exec_module(recovery)
with tempfile.TemporaryDirectory(prefix='p06-review-schema-') as directory:
    path = Path(directory).resolve() / 'manager.sqlite3'
    db = sqlite3.connect(path)
    os.chmod(path, 0o600)
    db.executescript('CREATE TABLE tasks(id TEXT PRIMARY KEY); CREATE TABLE vault(id INTEGER PRIMARY KEY,identity_ciphertext BLOB); INSERT INTO vault VALUES(1,x\'012345\'); INSERT INTO tasks VALUES(\'task\');')
    db.executescript((source / 'manager/migrations/0016.sql').read_text())
    db.execute('INSERT INTO credential_prepare_attempts VALUES(?,?)', ('task','execution'))
    db.commit()
    db.close()
    checked, version, identity = recovery.database(path, 'manager')
    assert version == 16
    assert checked.execute('SELECT * FROM credential_prepare_attempts').fetchall() == [('task','execution')]
    checked.execute('PRAGMA user_version=17')
    checked.close()
    try:
        recovery.database(path, 'manager')
    except ValueError as error:
        assert str(error) == 'unsupported database schema'
    else:
        raise AssertionError('schema17 accepted')
print('recovery_schema16_accepted=true; attempt_row_preserved=true; schema17_refused=true')
