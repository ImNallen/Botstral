import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from release_model import ROOT, authority, digest, filename, write_json
from release_update import (Busy, ChannelPointer, Effects, Idle, Probe, parse_latest,
                            parse_pointer, plan, rollback_method, run)


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

    def prepare(self, root, tamper=False, health=Probe.OK):
        key = root / 'key.pem'
        public = root / 'public.pem'
        subprocess.run(['openssl', 'genrsa', '-out', str(key), '2048'], check=True, capture_output=True)
        subprocess.run(['openssl', 'rsa', '-in', str(key), '-pubout', '-out', str(public)], check=True, capture_output=True)
        previous = self.identity('0.1.0', 'previous')
        target = self.identity('0.1.1', 'target')
        previous_dir = root / 'previous'
        target_dir = root / 'target'
        self.signed_set(previous_dir, previous, key)
        manifest = self.signed_set(target_dir, target, key)
        pointer = {'identity': target.json(), 'release': 'v0.1.1',
                   'manifest_sha256': digest(target_dir / 'manifest.json')}
        if tamper:
            (target_dir / 'manifest.json').write_text((target_dir / 'manifest.json').read_text() + '\n')
        state = root / 'state'
        state.mkdir()
        shutil.copytree(previous_dir, state / 'installed-release')
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

        effects = Effects(lambda _url: pointer, download, lambda: json.loads(version_path.read_text()),
                          lambda: {'idle': True}, lambda: calls['stop'].append('stop'),
                          lambda: calls['start'].append('start'), install, probe, lambda *_args: None,
                          lambda: 1_700_000_000)
        options = type('Options', (), {})()
        options.component = 'manager'
        options.state_dir = state
        options.channel_url = 'https://updates.example/channel-stable/channel.json'
        options.trusted_key = public
        options.report = state / 'update-report.json'
        options.listen = '127.0.0.1:43120'
        options.now = False
        return options, effects, calls, version_path, previous, target, manifest

    def report(self, options):
        return json.loads(options.report.read_text())

    def test_tampered_manifest_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, _target, _manifest = self.prepare(Path(directory), tamper=True)
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome'], {'type': 'rejected', 'reason': 'signature'})
            self.assertEqual(calls['install'], [])
            self.assertEqual(calls['stop'], [])
            self.assertEqual(json.loads(version_path.read_text())['identity'], previous.json())

    def test_idle_health_success_applies_release(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, target, _manifest = self.prepare(Path(directory))
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome'], {'type': 'applied', 'previous': previous.json(), 'current': target.json()})
            self.assertEqual(len(calls['install']), 1)
            self.assertEqual(calls['health'], ['127.0.0.1:43120'])
            self.assertEqual(json.loads(version_path.read_text())['identity'], target.json())
            self.assertEqual(report['installed']['identity'], target.json())

    def test_failed_health_rolls_back_to_previous_release(self):
        with tempfile.TemporaryDirectory() as directory:
            options, effects, calls, version_path, previous, target, manifest = self.prepare(Path(directory), health=Probe.FAIL)
            self.assertEqual(rollback_method(manifest, manifest, 'manager'), 'reinstall_package')
            self.assertEqual(run(options, effects), 0)
            report = self.report(options)
            self.assertEqual(report['last']['outcome']['type'], 'rolled_back')
            self.assertEqual(report['last']['outcome']['method'], 'reinstall_package')
            self.assertEqual(report['last']['outcome']['previous'], previous.json())
            self.assertEqual(report['last']['outcome']['failed'], target.json())
            self.assertEqual(json.loads(version_path.read_text())['identity'], previous.json())
            self.assertGreaterEqual(len(calls['install']), 2)
            self.assertTrue(calls['health'])


class DomainTests(unittest.TestCase):
    def test_plan_defers_a_busy_host_and_latest_json_stays_on_the_pointer(self):
        from release_model import ReleaseIdentity
        current = ReleaseIdentity.parse({'version': '0.1.0', 'channel': 'stable', 'revision': 'a' * 40, 'build_id': 'a'})
        newer = ReleaseIdentity.parse({'version': '0.1.1', 'channel': 'stable', 'revision': 'b' * 40, 'build_id': 'b'})
        self.assertEqual(plan(current, current, Idle()).json()['type'], 'current')
        deferred = plan(current, newer, Busy('active_task'))
        self.assertEqual(deferred.json()['type'], 'deferred')
        pointer = parse_pointer({'identity': newer.json(), 'release': 'v0.1.1', 'manifest_sha256': 'ab' * 32})
        self.assertIsInstance(pointer, ChannelPointer)
        latest = parse_latest({'version': newer.version, 'notes': 'Botstral ' + newer.version,
                               'pub_date': '2026-11-01T00:00:00Z', 'platforms': {}})
        self.assertEqual(latest['platforms'], {})


if __name__ == '__main__':
    unittest.main()
