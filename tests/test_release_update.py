import json
from dataclasses import replace
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from release_model import ROOT, authority, digest, filename, write_json
from release_update import (
    Effects, Probe, download_release, host_idle, pack_installed_deb,
    reinstall_previous, run, work_dir, write_atomic,
)


class UpdateTests(unittest.TestCase):
    def identity(self, version, build):
        from release_model import ReleaseIdentity
        return ReleaseIdentity.parse({'version': version, 'channel': 'stable', 'revision': 'a' * 40, 'build_id': build})

    def signed_set(self, root, identity, key):
        root.mkdir()
        config = authority()
        artifacts = []
        for entry in config['artifacts']:
            path = root / filename(entry, identity)
            path.write_bytes(b'layout fixture; not native acceptance')
            artifacts.append({'component': entry['component'], 'target': entry['target'],
                              'filename': path.name, 'sha256': digest(path), 'size': path.stat().st_size,
                              'identity': identity.json(), 'native_versions': identity.native_versions(),
                              'signing': 'unsigned'})
        value = {k: config[k] for k in ('format', 'protocols', 'data_compatibility', 'runtime_requirements')}
        value.update(identity=identity.json(), artifacts=artifacts, acceptance=[],
                     link_schema_sha256=digest(ROOT / 'link/schema/protocol.json'))
        write_json(root / 'manifest.json', value)
        subprocess.run(['openssl', 'dgst', '-sha256', '-sign', str(key), '-out', str(root / 'manifest.sig'),
                        str(root / 'manifest.json')], check=True, capture_output=True)
        return value

    def envelope(self, identity):
        config = authority()
        return {'component': 'manager', 'identity': identity.json(), 'protocols': config['protocols'],
                'link_schema_sha256': digest(ROOT / 'link/schema/protocol.json')}

    def prepare(self, root, tamper=False, health=Probe.OK, previous_set=True, component='manager'):
        key = root / 'key.pem'
        public = root / 'public.pem'
        subprocess.run(['openssl', 'genrsa', '-out', str(key), '2048'], check=True, capture_output=True)
        subprocess.run(['openssl', 'rsa', '-in', str(key), '-pubout', '-out', str(public)], check=True, capture_output=True)
        previous = self.identity('0.1.0', 'previous')
        target = self.identity('0.1.1', 'target')
        previous_dir = root / 'previous'
        target_dir = root / 'target'
        self.signed_set(previous_dir, previous, key)
        self.signed_set(target_dir, target, key)
        pointer = {'identity': target.json(), 'release': 'v0.1.1',
                   'manifest_sha256': digest(target_dir / 'manifest.json')}
        if tamper:
            (target_dir / 'manifest.json').write_text((target_dir / 'manifest.json').read_text() + '\n')
        state = root / 'state'
        state.mkdir()
        if previous_set:
            workspace = work_dir(state)
            workspace.mkdir()
            shutil.copytree(previous_dir, workspace / 'installed-release')
        install_root = root / 'installed'
        install_root.mkdir()
        version_path = install_root / 'version.json'
        write_json(version_path, self.envelope(previous))
        calls = {'install': [], 'health': [], 'stop': [], 'start': []}

        def download(_url, _pointer, directory):
            if directory.exists():
                shutil.rmtree(directory)
            shutil.copytree(target_dir, directory)

        def install(release_dir, _component, _version, _backup, _config, _receipt):
            calls['install'].append(release_dir)
            identity = json.loads((release_dir / 'manifest.json').read_text())['identity']
            write_json(version_path, self.envelope(type(previous).parse(identity)))

        def probe(_listen):
            calls['health'].append(_listen)
            return health

        def reinstall(release_dir, component, version):
            install(release_dir, component, version, None, None, None)

        effects = Effects(lambda _url: pointer, download, lambda: json.loads(version_path.read_text()),
                          lambda: {'idle': True},
                          lambda: calls['stop'].append('stop'),
                          lambda: calls['start'].append('start'), install, reinstall, probe, lambda *_args: None,
                          lambda: 1_700_000_000)
        options = type('Options', (), {})()
        options.component = component
        options.state_dir = state
        options.channel_url = 'https://updates.example/channel-stable/channel.json'
        options.trusted_key = public
        options.report = state / 'update-report.json'
        options.listen = '127.0.0.1:43120'
        options.now = False
        return options, effects, calls, version_path, previous, target

    def report(self, options):
        return json.loads(options.report.read_text())

    def test_tampered_manifest_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, _target = self.prepare(Path(directory), tamper=True)
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome'], {'type': 'rejected', 'reason': 'signature'})
            self.assertEqual(calls['install'], [])
            self.assertEqual(calls['stop'], [])
            self.assertEqual(json.loads(version_path.read_text())['identity'], previous.json())

    def test_idle_health_success_applies_release(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, target = self.prepare(Path(directory))
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome'], {'type': 'applied', 'previous': previous.json(), 'current': target.json()})
            self.assertEqual(len(calls['install']), 1)
            self.assertEqual(calls['health'], ['127.0.0.1:43120'])
            self.assertEqual(json.loads(version_path.read_text())['identity'], target.json())
            self.assertEqual(report['installed']['identity'], target.json())

    def test_failed_health_rolls_back_to_previous_release(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, target = self.prepare(Path(directory), health=Probe.FAIL)
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome']['type'], 'rolled_back')
            self.assertEqual(report['last']['outcome']['method'], 'reinstall_package')
            self.assertEqual(report['last']['outcome']['previous'], previous.json())
            self.assertEqual(report['last']['outcome']['failed'], target.json())
            self.assertEqual(json.loads(version_path.read_text())['identity'], previous.json())
            self.assertGreaterEqual(len(calls['install']), 2)
            self.assertTrue(calls['health'])

    def test_first_update_failed_health_rolls_back(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, target = self.prepare(
                Path(directory), previous_set=False, health=Probe.FAIL)
            self.assertFalse((work_dir(options.state_dir) / 'installed-release' / 'manifest.json').is_file())
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome']['type'], 'rolled_back')
            self.assertEqual(report['last']['outcome']['method'], 'reinstall_package')
            self.assertEqual(report['last']['outcome']['previous'], previous.json())
            self.assertEqual(report['last']['outcome']['failed'], target.json())
            self.assertEqual(json.loads(version_path.read_text())['identity'], previous.json())
            self.assertGreaterEqual(len(calls['install']), 2)
            self.assertTrue(calls['start'])

    def test_first_update_without_installed_release_applies(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, target = self.prepare(
                Path(directory), previous_set=False)
            self.assertFalse((work_dir(options.state_dir) / 'installed-release' / 'manifest.json').is_file())
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome'], {
                'type': 'applied', 'previous': previous.json(), 'current': target.json()})
            self.assertEqual(len(calls['install']), 1)
            self.assertEqual(json.loads(version_path.read_text())['identity'], target.json())
            self.assertTrue((work_dir(options.state_dir) / 'installed-release' / 'manifest.json').is_file())

    def test_first_update_keeps_host_state_private(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, _previous, target = self.prepare(
                Path(directory), previous_set=False)
            options.state_dir.chmod(0o700)
            leftover = options.state_dir / 'update-incoming'
            leftover.mkdir()
            leftover.chmod(0o700)
            self.assertEqual(run(options, effects), 0)
            self.assertEqual(self.report(options)['last']['outcome']['type'], 'applied')
            self.assertEqual(json.loads(version_path.read_text())['identity'], target.json())
            for name in ('update-incoming', 'update-backup', 'installed-release'):
                self.assertFalse((options.state_dir / name).exists(), name)
            workspace = work_dir(options.state_dir)
            self.assertTrue((workspace / 'installed-release' / 'manifest.json').is_file())
            report = options.state_dir / 'update-report.json'
            self.assertEqual(report.stat().st_mode & 0o7777, 0o600)
            self.assertEqual(report.stat().st_uid, options.state_dir.stat().st_uid)
            self.assertEqual(len(calls['start']), 1)

    def test_rollback_clears_leftover_package_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            release = root / 'release'
            release.mkdir()
            write_json(release / 'manifest.json', {
                'artifacts': [{'component': 'manager', 'filename': 'botstral-manager_0.1.0_amd64.deb'}],
            })
            (release / 'botstral-manager_0.1.0_amd64.deb').write_bytes(b'pkg')
            leftover = root / 'usr' / 'lib' / 'botstral-manager' / 'tools' / 'scripts' / '__pycache__'
            leftover.mkdir(parents=True)
            (leftover / 'release_model.cpython-312.pyc').write_bytes(b'x')
            seen = []

            def fake_run(command, check=True, **_kwargs):
                seen.append(command[:2])
                result = subprocess.CompletedProcess(command, 0)
                if command[:2] == ['dpkg', '--install'] and (root / 'usr' / 'lib' / 'botstral-manager').exists():
                    raise subprocess.CalledProcessError(1, command, 'existing unmanaged path')
                return result

            original = subprocess.run
            subprocess.run = fake_run
            try:
                reinstall_previous(release, 'manager', root)
            finally:
                subprocess.run = original
            self.assertEqual(seen, [['dpkg', '--remove'], ['dpkg', '--install']])
            self.assertFalse((root / 'usr' / 'lib' / 'botstral-manager').exists())

    def test_pack_installed_deb_with_private_umask(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'botstral-agent'
            binary.write_bytes(b'x')
            destination = root / 'installed-release'
            destination.mkdir()
            status = (
                'Package: botstral-agent\nVersion: 0.1.0\nArchitecture: amd64\n'
                'Maintainer: test\nDescription: test\n'
            )

            def fake_output(command, text=True, stderr=None):
                if command[:2] == ['dpkg-query', '-s']:
                    return status
                if command[:2] == ['dpkg-query', '-L']:
                    return str(binary) + '\n'
                raise subprocess.CalledProcessError(1, command)

            previous = os.umask(0o077)
            original = subprocess.check_output
            subprocess.check_output = fake_output
            try:
                name = pack_installed_deb('agent', destination)
            finally:
                subprocess.check_output = original
                os.umask(previous)
            self.assertEqual(name, 'botstral-agent_0.1.0_amd64.deb')
            self.assertTrue((destination / name).is_file())

    def test_worker_update_applies_when_idle(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, target = self.prepare(
                Path(directory), component='agent')
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome'], {
                'type': 'applied', 'previous': previous.json(), 'current': target.json()})
            self.assertEqual(len(calls['install']), 1)
            self.assertEqual(json.loads(version_path.read_text())['identity'], target.json())

    def test_pre_install_failure_does_not_stop_the_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, _target = self.prepare(
                Path(directory), tamper=True, component='agent')
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome'], {'type': 'rejected', 'reason': 'signature'})
            self.assertEqual(calls['install'], [])
            self.assertEqual(calls['stop'], [])
            self.assertEqual(json.loads(version_path.read_text())['identity'], previous.json())

    def test_busy_host_is_deferred(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, _target = self.prepare(Path(directory))
            db = sqlite3.connect(options.state_dir / 'manager.sqlite3')
            db.execute('CREATE TABLE tasks (view TEXT)')
            db.execute("INSERT INTO tasks(view) VALUES ('{\"state\":\"running\"}')")
            db.commit()
            db.close()
            effects = replace(effects, idle=lambda: host_idle('manager', options.state_dir))
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome'], {
                'type': 'deferred', 'busy': {'state': 'busy', 'reason': 'active_task'}})
            self.assertEqual(calls['install'], [])
            self.assertEqual(calls['stop'], [])
            self.assertEqual(json.loads(version_path.read_text())['identity'], previous.json())


class DownloadSafetyTests(unittest.TestCase):
    def test_unsigned_manifest_does_not_fetch_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            options, _effects, _calls, _version, _previous, _target = UpdateTests().prepare(
                root, tamper=True)
            fetched = []

            def fetch(url):
                fetched.append(url.rsplit('/', 1)[-1])
                name = url.rsplit('/', 1)[-1]
                return (root / 'target' / name).read_bytes()

            incoming = root / 'incoming'
            with self.assertRaises(subprocess.CalledProcessError):
                download_release(options.channel_url, type('P', (), {
                    'release': 'v0.1.1',
                })(), incoming, fetch, options.trusted_key)
            self.assertEqual(fetched, ['manifest.json', 'manifest.sig'])
            self.assertFalse(any(incoming.glob('botstral-*')))

    def test_report_matches_state_directory_owner(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            report = state / 'update-report.json'
            write_atomic(report, {'installed': {}, 'last': None})
            self.assertEqual(report.stat().st_uid, state.stat().st_uid)
            self.assertEqual(report.stat().st_gid, state.stat().st_gid)
            self.assertEqual(report.stat().st_mode & 0o777, 0o600)


if __name__ == '__main__':
    unittest.main()
