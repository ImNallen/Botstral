import copy
import fcntl
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch
import uuid

import recovery as r


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.state = self.root / 'state'
        self.state.mkdir(mode=0o700)
        (self.state / 'manager.lock').touch(mode=0o600)
        self.db = sqlite3.connect(self.state / 'manager.sqlite3')
        self.addCleanup(self.db.close)
        self.db.executescript("PRAGMA journal_mode=WAL; PRAGMA user_version=8; CREATE TABLE vault(id INTEGER,identity_ciphertext BLOB); INSERT INTO vault VALUES(1,x'010203'); CREATE TABLE artifacts(id TEXT,offer TEXT,state TEXT); CREATE TABLE history(body TEXT); INSERT INTO history VALUES('durable owner history');")
        self.db.commit()
        self.private()
        (self.state / 'memory-test.md').write_text('authoritative memory')
        (self.state / 'memory-test.md').chmod(0o600)
        self.backup = self.root / 'backup'

    def private(self):
        for path in self.state.iterdir():
            path.chmod(0o600)

    def create(self):
        return r.create('manager', self.state, self.backup)

    def test_wal_restore_and_locked_state(self):
        self.assertGreater((self.state / 'manager.sqlite3-wal').stat().st_size, 0)
        identity = self.create()
        self.assertEqual(r.verify('manager', self.backup).backup_id, identity)
        restored = self.root / 'restored'
        r.restore('manager', self.backup, restored)
        with sqlite3.connect((restored / 'manager.sqlite3').as_uri() + '?immutable=1', uri=True) as db:
            self.assertEqual(db.execute('SELECT body FROM history').fetchall(), [('durable owner history',)])
            self.assertEqual(db.execute('SELECT identity_ciphertext FROM vault').fetchone()[0], b'\x01\x02\x03')
        self.assertEqual((restored / 'memory-test.md').read_text(), 'authoritative memory')
        self.assertFalse((restored / 'manager.sqlite3-wal').exists())
        with self.assertRaisesRegex(ValueError, 'must be new'):
            r.restore('manager', self.backup, restored)

    def test_flock_matches_service(self):
        with open(self.state / 'manager.lock', 'rb') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaises(BlockingIOError):
                self.create()
        self.assertFalse(self.backup.exists())

    def test_available_ledger_bytes_are_required(self):
        identity = str(uuid.uuid4())
        content = b'durable deliverable'
        blobs = self.state / 'artifacts'
        blobs.mkdir(mode=0o700)
        blob = blobs / (identity + '.blob')
        blob.write_bytes(content)
        blob.chmod(0o600)
        (blobs / 'unfinished.part').write_bytes(b'partial')
        (blobs / 'unfinished.part').chmod(0o600)
        offer = {'output': {'type': 'content', 'content': {'length': len(content), 'sha256': r.digest(blob)}}}
        self.db.execute('INSERT INTO artifacts VALUES(?,?,?)', (identity, json.dumps(offer), 'available'))
        self.db.commit()
        self.create()
        self.assertFalse((self.backup / 'files/state/artifacts/unfinished.part').exists())
        blob.write_bytes(b'corrupt')
        with self.assertRaisesRegex(ValueError, 'artifact integrity'):
            r.create('manager', self.state, self.root / 'corrupt-backup')

    def test_link_and_source_change_refusals(self):
        target = self.state / 'memory-test.md'
        for link_kind in ('symlink', 'hardlink'):
            link = self.state / 'unsafe'
            if link_kind == 'symlink':
                link.symlink_to(target)
            else:
                os.link(target, link)
            with self.assertRaises(ValueError):
                self.create()
            link.unlink()
        copy_file = r.copy_file
        def changing(source, destination):
            copy_file(source, destination)
            if source.name == 'memory-test.md':
                source.write_text('concurrent mutation')
        with patch.object(r, 'copy_file', changing):
            with self.assertRaisesRegex(ValueError, 'source changed'):
                self.create()
        self.assertFalse(self.backup.exists())

    def test_backup_links_and_extra_content_are_refused(self):
        self.create()
        memory = self.backup / 'files/state/memory-test.md'
        original = self.root / 'retained-memory'
        memory.rename(original)
        memory.symlink_to(original)
        with self.assertRaises(ValueError):
            r.verify('manager', self.backup)
        memory.unlink()
        os.link(original, memory)
        with self.assertRaises(ValueError):
            r.verify('manager', self.backup)
        memory.unlink()
        original.rename(memory)
        extra = self.backup / 'files/state/unlisted'
        extra.write_text('unexpected content')
        with self.assertRaisesRegex(ValueError, 'unexpected'):
            r.verify('manager', self.backup)

    def test_future_database_schema_is_refused(self):
        self.db.execute('PRAGMA user_version=999')
        with self.assertRaisesRegex(ValueError, 'unsupported database'):
            self.create()
        self.assertFalse(self.backup.exists())

    def test_malicious_manifest_refusals_before_destination(self):
        self.create()
        manifest_path = self.backup / 'manifest.json'
        original = r.read_json(manifest_path)
        mutations = []
        bad = copy.deepcopy(original)
        bad['format'] = 999
        mutations.append(bad)
        bad = copy.deepcopy(original)
        bad['entries'][1]['path'] = '../outside'
        mutations.append(bad)
        bad = copy.deepcopy(original)
        bad['schema'] = 999
        mutations.append(bad)
        bad = copy.deepcopy(original)
        bad['entries'].append(bad['entries'][1])
        mutations.append(bad)
        bad = copy.deepcopy(original)
        bad['identity'] = 'wrong'
        mutations.append(bad)
        for mutation in mutations:
            manifest_path.write_text(json.dumps(mutation))
            with self.assertRaises(ValueError):
                r.restore('manager', self.backup, self.root / 'restored')
            self.assertFalse((self.root / 'restored').exists())
        manifest_path.write_text(json.dumps(original))
        memory = self.backup / 'files/state/memory-test.md'
        memory.write_text('tamper')
        with self.assertRaisesRegex(ValueError, 'integrity'):
            r.verify('manager', self.backup)


class WorkerRecoveryTests(unittest.TestCase):
    def test_selected_guest_roots_identity_evidence_and_no_clone(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            state = root / 'state'
            state.mkdir(mode=0o700)
            (state / 'agent.lock').touch(mode=0o600)
            incarnation = str(uuid.uuid4())
            with sqlite3.connect(state / 'agent.sqlite3') as db:
                db.executescript('PRAGMA user_version=2; CREATE TABLE metadata(incarnation TEXT); CREATE TABLE executions(record TEXT);')
                db.execute('INSERT INTO metadata VALUES(?)', (incarnation,))
                db.execute('INSERT INTO executions VALUES(?)', (json.dumps({'process': {'lifecycle': {'type': 'stopped'}}}),))
            for path in state.iterdir():
                path.chmod(0o600)
            selected = []
            for kind in ('workspace', 'browser-profile', 'coding-home', 'certificate', 'runtime', 'tool'):
                path = root / kind
                path.mkdir(mode=0o700)
                (path / 'bytes').write_text(kind + ' retained bytes')
                (path / 'bytes').chmod(0o700 if kind == 'tool' else 0o600)
                selected.append({'label': kind, 'kind': kind, 'path': str(path)})
            internal_certificates = state / 'certificates'
            internal_certificates.mkdir(mode=0o700)
            certificate = internal_certificates / 'identity.pem'
            certificate.write_text('retained worker identity certificate')
            certificate.chmod(0o600)
            (state / 'config.json').write_text(json.dumps({'manager_ca': str(certificate), 'client_cert': str(certificate), 'client_key': str(certificate)}))
            (state / 'config.json').chmod(0o600)
            processes = state / 'processes'
            processes.mkdir(mode=0o700)
            (processes / 'retained-evidence').write_text('scope stop evidence')
            (processes / 'retained-evidence').chmod(0o600)
            interpreter = root / 'runtime/bytes'
            (state / 'connector.json').write_text(json.dumps({'python': str(interpreter), 'python_sha256': r.digest(interpreter)}))
            (state / 'connector.json').chmod(0o600)
            self.assertIn(str(interpreter), r.references(state))
            omitted_runtime = [item for item in selected if item['kind'] != 'runtime']
            with self.assertRaisesRegex(ValueError, 'runtime reference is not covered'):
                r.create('agent', state, root / 'missing-runtime', {'roots': omitted_runtime})
            self.assertFalse((root / 'missing-runtime').exists())
            with patch.object(r, 'worker_quiet') as quiet:
                r.create('agent', state, root / 'backup', {'roots': selected})
                self.assertEqual(quiet.call_count, 2)
            with self.assertRaisesRegex(ValueError, 'retired'):
                r.restore('agent', root / 'backup', root / 'restored')
            with open(state / 'agent.lock', 'rb') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                with self.assertRaises(BlockingIOError):
                    r.restore('agent', root / 'backup', root / 'restored', retired=True)
            for selected_root in selected:
                Path(selected_root['path']).rename(selected_root['path'] + '-retired')
            state.rename(root / 'retired-state')
            with patch.object(r, 'worker_quiet'):
                with self.assertRaisesRegex(ValueError, 'original state path'):
                    r.restore('agent', root / 'backup', root / 'restored', retired=True)
                self.assertFalse((root / 'restored').exists())
                r.restore('agent', root / 'backup', state, retired=True)
            self.assertEqual((state / 'processes/retained-evidence').read_text(), 'scope stop evidence')
            restored_config = r.read_json(state / 'config.json')
            self.assertEqual(Path(restored_config['client_cert']).read_text(), 'retained worker identity certificate')
            for selected_root in selected:
                self.assertEqual((Path(selected_root['path']) / 'bytes').read_text(), selected_root['kind'] + ' retained bytes')
            self.assertEqual((root / 'tool/bytes').stat().st_mode & 0o777, 0o700)
            import diagnose
            self.assertEqual(diagnose.runtime_hashes(state), ['connector.json'])
            self.assertEqual(interpreter.read_text(), 'runtime retained bytes')
            with sqlite3.connect(state / 'agent.sqlite3') as db:
                self.assertEqual(db.execute('SELECT incarnation FROM metadata').fetchone()[0], incarnation)
            (state / 'agent.lock').touch(mode=0o600)
            with sqlite3.connect(state / 'agent.sqlite3') as db:
                db.execute('UPDATE executions SET record=?', (json.dumps({'process': {'lifecycle': {'type': 'launch_intent'}}}),))
            with patch.object(r, 'worker_quiet'):
                with self.assertRaisesRegex(ValueError, 'unproved'):
                    r.create('agent', state, root / 'unsafe', {'roots': selected})


if __name__ == '__main__':
    os.umask(0o077)
    unittest.main()
