import copy
import fcntl
import json
import os
from pathlib import Path
import sqlite3
import ssl
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

    def test_schema_11_restores_frozen_run_memory_and_immutable_guard(self):
        self.db.executescript("CREATE TABLE runs(id TEXT PRIMARY KEY,owner TEXT,request TEXT,configuration TEXT,summary TEXT); CREATE TRIGGER immutable_run BEFORE UPDATE OF id ON runs BEGIN SELECT RAISE(ABORT, 'immutable run'); END;")
        migration = Path(__file__).resolve().parents[1] / 'migrations/0011.sql'
        self.db.executescript(migration.read_text())
        memory = json.dumps({'bot': str(uuid.uuid4()), 'kind': 'memory', 'revision': 2,
                             'sources': [{'type': 'external', 'reference': 'https://example.com/preference'}],
                             'markdown': 'Use the corrected preference.'})
        self.db.execute("INSERT INTO runs(id,owner,request,configuration,summary,memory_snapshot) VALUES('frozen','owner','{}','{}','{}',?)", (memory,))
        self.db.execute("INSERT INTO runs(id,owner,request,configuration,summary,memory_snapshot) VALUES('legacy','owner','{}','{}','{}',NULL)")
        self.db.commit()
        self.private()
        self.create()
        restored = self.root / 'restored'
        r.restore('manager', self.backup, restored)
        with sqlite3.connect(restored / 'manager.sqlite3') as db:
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0], 11)
            self.assertEqual(db.execute('SELECT id,memory_snapshot FROM runs ORDER BY id').fetchall(),
                             [('frozen', memory), ('legacy', None)])
            with self.assertRaisesRegex(sqlite3.IntegrityError, 'immutable run'):
                db.execute("UPDATE runs SET memory_snapshot=NULL WHERE id='frozen'")

    def test_flock_matches_service(self):
        with open(self.state / 'manager.lock', 'rb') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaises(BlockingIOError):
                self.create()
        self.assertFalse(self.backup.exists())

    def test_schema_12_keeps_pending_publication_and_distinguishes_empty_from_legacy_snapshots(self):
        self.db.executescript("CREATE TABLE runs(id TEXT PRIMARY KEY,owner TEXT,request TEXT,configuration TEXT,summary TEXT); CREATE TRIGGER immutable_run BEFORE UPDATE OF id ON runs BEGIN SELECT RAISE(ABORT, 'immutable run'); END;")
        migrations = Path(__file__).resolve().parents[1] / 'migrations'
        for version in (11, 12):
            self.db.executescript((migrations / f'{version:04}.sql').read_text())
        self.db.execute("INSERT INTO runs VALUES('new','owner','{}','{}','{}',NULL,'[]')")
        self.db.execute("INSERT INTO runs VALUES('legacy','owner','{}','{}','{}','legacy frozen document',NULL)")
        self.db.execute("INSERT INTO memory_publications VALUES('turn','new','attempt','call','entry',NULL,?,'receipt','pending')", (b'exact intended bytes',))
        self.db.commit()
        self.private()
        self.create()
        restored = self.root / 'restored'
        r.restore('manager', self.backup, restored)
        with sqlite3.connect(restored / 'manager.sqlite3') as db:
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0], 12)
            self.assertEqual(db.execute('SELECT memory_snapshot,memory_entries_snapshot FROM runs ORDER BY id').fetchall(), [('legacy frozen document', None), (None, '[]')])
            self.assertEqual(db.execute('SELECT intended_bytes,state FROM memory_publications').fetchall(), [(b'exact intended bytes', 'pending')])
            with self.assertRaisesRegex(sqlite3.IntegrityError, 'immutable run'):
                db.execute("UPDATE runs SET memory_entries_snapshot='[]' WHERE id='legacy'")
            with self.assertRaisesRegex(sqlite3.IntegrityError, 'immutable memory publication'):
                db.execute("UPDATE memory_publications SET intended_bytes=x'00'")

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

    def attachments(self):
        self.db.executescript((Path(__file__).resolve().parents[1] / 'migrations/0017.sql').read_text())
        directory = self.state / 'run-files'
        directory.mkdir(mode=0o700)
        files = []
        for table, content in [('run_files', b'bound attachment'),
                               ('staged_run_files', b'\xff\x00staged attachment')]:
            identity = str(uuid.uuid4())
            path = directory / identity
            path.write_bytes(content)
            path.chmod(0o600)
            self.db.execute(f'INSERT INTO {table} VALUES(?,?,?,?)',
                            (identity, 'note.txt', r.digest(path), len(content)))
            files.append((table, identity, content))
        self.db.commit()
        self.private_database()
        return files

    def private_database(self):
        for path in self.state.glob('manager.sqlite3*'):
            path.chmod(0o600)

    def test_bound_and_staged_attachments_restore_with_private_verified_bytes(self):
        files = self.attachments()
        self.create()
        restored = self.root / 'restored'
        r.restore('manager', self.backup, restored)
        self.assertEqual((restored / 'run-files').stat().st_mode & 0o777, 0o700)
        with sqlite3.connect(restored / 'manager.sqlite3') as db:
            for table, identity, content in files:
                path = restored / 'run-files' / identity
                self.assertEqual(path.read_bytes(), content)
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertEqual(db.execute(f'SELECT sha256,length FROM {table} WHERE run=?', (identity,)).fetchone(),
                                 (r.digest(path), len(content)))

    def test_attachment_ledger_refuses_missing_corrupt_and_linked_bytes(self):
        files = self.attachments()
        for table, identity, content in files:
            path = self.state / 'run-files' / identity
            for case in ('missing', 'corrupt', 'symlink', 'hardlink'):
                with self.subTest(table=table, case=case):
                    path.unlink()
                    if case == 'corrupt':
                        path.write_bytes(b'wrong attachment')
                        path.chmod(0o600)
                    elif case == 'symlink':
                        path.symlink_to(self.state / 'memory-test.md')
                    elif case == 'hardlink':
                        os.link(self.state / 'memory-test.md', path)
                    with self.assertRaises((ValueError, FileNotFoundError)):
                        self.create()
                    self.assertFalse(self.backup.exists())
                    path.unlink(missing_ok=True)
                    path.write_bytes(content)
                    path.chmod(0o600)

    def test_attachment_manifest_cannot_override_database_digest(self):
        _, identity, _ = self.attachments()[0]
        self.create()
        path = self.backup / 'files/state/run-files' / identity
        path.write_bytes(b'replacement bytes')
        manifest_path = self.backup / 'manifest.json'
        manifest = r.read_json(manifest_path)
        for item in manifest['entries']:
            if item['path'] == 'run-files/' + identity:
                item.update(size=path.stat().st_size, sha256=r.digest(path))
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, 'attachment integrity'):
            r.restore('manager', self.backup, self.root / 'restored')
        self.assertFalse((self.root / 'restored').exists())

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
            (state / 'config.json').write_text(json.dumps({'manager': 'wss://127.0.0.1:1', 'pin': '0' * 64, 'client_cert': str(certificate), 'client_key': str(certificate)}))
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

    def test_pin_worker_config_is_diagnosable_without_manager_ca(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            state = root / 'state'
            state.mkdir(mode=0o700)
            (state / 'agent.lock').touch(mode=0o600)
            certificate = root / 'identity.pem'
            key = root / 'identity.key'
            certificate.write_text('retained worker identity certificate')
            key.write_text('retained worker identity key')
            certificate.chmod(0o600)
            key.chmod(0o600)
            config = {
                'manager': 'wss://127.0.0.1:1',
                'pin': '0' * 64,
                'client_cert': str(certificate),
                'client_key': str(key),
            }
            (state / 'config.json').write_text(json.dumps(config))
            (state / 'config.json').chmod(0o600)
            try:
                refs = r.references(state)
            except KeyError as error:
                self.fail('references still requires ' + str(error))
            self.assertEqual(set(refs), {str(certificate), str(key)})
            import diagnose
            try:
                diagnose.connectivity(config)
            except KeyError as error:
                self.fail('diagnose still requires ' + str(error))
            except (OSError, ValueError, ssl.SSLError):
                pass


if __name__ == '__main__':
    os.umask(0o077)
    unittest.main()
