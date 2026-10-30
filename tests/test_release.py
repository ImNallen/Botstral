import copy
import dataclasses
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from release_model import (ROOT, ReleaseIdentity, ReleaseManifest, authority, digest,
                           embedded, filename, verify, write_json)
from release import versions


class ReleaseTests(unittest.TestCase):
    def identity(self, nightly=False):
        return ReleaseIdentity.parse({'version': '0.1.0-nightly.20261030.1' if nightly else '0.1.0',
                                      'channel': 'nightly' if nightly else 'stable',
                                      'revision': 'a' * 40, 'build_id': 'fixture-1'})

    def manifest(self, root):
        config = authority()
        identity = self.identity()
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
        return value

    def test_source_versions(self):
        versions()

    def test_native_mapping_and_identity(self):
        self.assertEqual(self.identity(True).native_versions(), {'debian': '0.1.0~nightly.20261030.1-1', 'macos_short': '0.1.0', 'macos_bundle': '0.1.0d1'})
        self.assertEqual(self.identity().native_versions()['debian'], '0.1.0-1')
        for replacement in ({'channel': 'nightly'}, {'revision': 'short'}, {'version': '0.1.0/escape'}, {'build_id': '../escape'}):
            with self.subTest(replacement=replacement), self.assertRaises(ValueError):
                ReleaseIdentity.parse(dict(self.identity().json(), **replacement))

    def test_exact_set_and_digest_refusals(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value = self.manifest(root)
            verify(root, unsigned=True)
            for field, replacement in [('artifacts', value['artifacts'][:-1]), ('artifacts', value['artifacts'] + [value['artifacts'][0]]), ('protocols', {'worker': 99, 'owner': 3})]:
                changed = copy.deepcopy(value)
                changed[field] = replacement
                with self.subTest(field=field), self.assertRaises(ValueError):
                    ReleaseManifest.parse(changed)
            for field, replacement in [('identity', self.identity(True).json()), ('target', 'wrong'), ('filename', '../escape'), ('native_versions', {})]:
                changed = copy.deepcopy(value)
                changed['artifacts'][0][field] = replacement
                with self.subTest(field=field), self.assertRaises(ValueError):
                    ReleaseManifest.parse(changed)
            with self.assertRaisesRegex(ValueError, 'unsigned mode'):
                verify(root, unsigned=True, production=True)
            with self.assertRaisesRegex(ValueError, 'native acceptance'):
                ReleaseManifest.parse(value, production=True)
            path = root / value['artifacts'][0]['filename']
            original = path.read_bytes()
            path.write_bytes(b'corrupt')
            with self.assertRaisesRegex(ValueError, 'corrupt'):
                verify(root, unsigned=True)
            path.unlink()
            with self.assertRaisesRegex(ValueError, 'missing'):
                verify(root, unsigned=True)
            elsewhere = root.parent / (root.name + '-symlink')
            elsewhere.write_bytes(original)
            try:
                path.symlink_to(elsewhere)
                with self.assertRaisesRegex(ValueError, 'unsafe'):
                    verify(root, unsigned=True)
            finally:
                elsewhere.unlink()

    def test_independent_signature_and_tampering(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            release = root / 'release'
            release.mkdir()
            self.manifest(release)
            key = root / 'key.pem'
            public = root / 'public.pem'
            subprocess.run(['openssl', 'genrsa', '-out', str(key), '2048'], check=True, capture_output=True)
            subprocess.run(['openssl', 'rsa', '-in', str(key), '-pubout', '-out', str(public)], check=True, capture_output=True)
            subprocess.run(['openssl', 'dgst', '-sha256', '-sign', str(key), '-out', str(release / 'manifest.sig'), str(release / 'manifest.json')], check=True, capture_output=True)
            verify(release, public)
            with self.assertRaisesRegex(ValueError, 'independently'):
                verify(release)
            with self.assertRaisesRegex(ValueError, 'outside'):
                verify(release, release / 'key.pem')
            (release / 'manifest.json').write_text((release / 'manifest.json').read_text() + ' ')
            with self.assertRaises(subprocess.CalledProcessError):
                verify(release, public)

    def test_embedded_real_binary_if_built(self):
        path = ROOT / 'target/debug/botstral-manager'
        if not path.exists():
            self.skipTest('manager not built')
        expected = embedded(path)
        actual = json.loads(subprocess.check_output([str(path), '--version-json']))
        self.assertEqual(actual.pop('component'), 'manager')
        self.assertEqual(actual, expected)


if __name__ == '__main__':
    unittest.main()
