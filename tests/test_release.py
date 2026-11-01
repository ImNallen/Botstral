import copy
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
            wrong = root / 'wrong.pem'
            wrong_public = root / 'wrong-public.pem'
            subprocess.run(['openssl', 'genrsa', '-out', str(wrong), '2048'], check=True, capture_output=True)
            subprocess.run(['openssl', 'rsa', '-in', str(wrong), '-pubout', '-out', str(wrong_public)], check=True, capture_output=True)
            with self.assertRaises(subprocess.CalledProcessError):
                verify(release, wrong_public)

            with self.assertRaisesRegex(ValueError, 'independently'):
                verify(release)
            with self.assertRaisesRegex(ValueError, 'outside'):
                verify(release, release / 'key.pem')
            (release / 'manifest.json').write_text((release / 'manifest.json').read_text() + ' ')
            with self.assertRaises(subprocess.CalledProcessError):
                verify(release, public)
            wrong = root / 'wrong.pem'
            wrong_public = root / 'wrong-public.pem'
            subprocess.run(['openssl', 'genrsa', '-out', str(wrong), '2048'], check=True, capture_output=True)
            subprocess.run(['openssl', 'rsa', '-in', str(wrong), '-pubout', '-out', str(wrong_public)], check=True, capture_output=True)
            with self.assertRaises(subprocess.CalledProcessError):
                verify(release, wrong_public)


    def test_embedded_real_binaries_if_built(self):
        import os
        names = ('manager', 'agent', 'agent-supervisor-check')
        for name in names:
            path = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')) / 'debug' / ('botstral-' + name)
            if not path.exists():
                self.skipTest('build production binaries before release tests')
            expected = embedded(path)
            actual = json.loads(subprocess.check_output([str(path), '--version-json']))
            self.assertEqual(actual.pop('component'), name)
            self.assertEqual(actual, expected)
            self.assertEqual(actual['protocols'], authority()['protocols'])
            self.assertEqual(actual['link_schema_sha256'], digest(ROOT / 'link/schema/protocol.json'))
            self.assertEqual(actual['identity']['revision'], os.environ.get('BOTSTRAL_SOURCE_REVISION') or subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip())


class VersionDriftTests(unittest.TestCase):
    def test_build_rejects_revision_override_before_compilation(self):
        import os
        from unittest.mock import patch
        from release_package import build
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identity = ReleaseTests().identity(True)
            path = root / 'identity.json'
            write_json(path, identity.json())
            with patch.dict(os.environ, {'BOTSTRAL_SOURCE_REVISION': identity.revision}):
                with self.assertRaisesRegex(ValueError, 'source revision override differs from checkout'):
                    build(path, 'macos-arm64', root / 'output', unsigned=False)
            self.assertFalse((root / 'output').exists())

    def test_every_source_mirror_refuses_drift(self):
        import re
        import release
        import shutil
        files = ['manager/Cargo.toml', 'agent/Cargo.toml', 'link/Cargo.toml',
                 'link/client/Cargo.toml', 'desktop/src-tauri/Cargo.toml',
                 'desktop/package.json', 'link/package.json', 'desktop/src-tauri/tauri.conf.json',
                 'package-lock.json', 'Cargo.lock', 'desktop/src-tauri/Cargo.lock']
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in files + ['link/src/lib.rs', 'link/src/owner.rs']:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, path)
            previous = release.ROOT
            release.ROOT = root
            try:
                for name in files:
                    path = root / name
                    original = path.read_text()
                    if name.endswith('.lock'):
                        changed = re.sub(r'(name = "botstral-link"\nversion = ")[^"]+', r'\g<1>9.9.9', original)
                    else:
                        changed = original.replace(authority()['version'], '9.9.9', 1)
                    self.assertNotEqual(original, changed, name)
                    path.write_text(changed)
                    with self.subTest(file=name), self.assertRaisesRegex(ValueError, 'version drift'):
                        release.versions()
                    path.write_text(original)
                value = json.loads((root / 'desktop/package.json').read_text())
                value['dependencies']['@botstral/link'] = '9.9.9'
                write_json(root / 'desktop/package.json', value)
                with self.assertRaisesRegex(ValueError, 'version drift'):
                    release.versions()
            finally:
                release.ROOT = previous


class PayloadPolicyTests(unittest.TestCase):
    def test_unexpected_paths_links_and_privileged_modes_are_refused(self):
        import tarfile
        from release_artifacts import validate_payload
        class Archive:
            def __init__(self, member):
                self.member = member
            def getmembers(self):
                return [self.member]
        for path in ('../escape', '/usr/bin/botstral-manager', './etc/botstral-manager/service.json', './usr/bin/unrelated'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                validate_payload(Archive(tarfile.TarInfo(path)), 'manager')
        member = tarfile.TarInfo('./usr/lib/botstral-manager/unsafe')
        member.type = tarfile.SYMTYPE
        member.linkname = '/var/lib/botstral-manager'
        with self.assertRaisesRegex(ValueError, 'symlink'):
            validate_payload(Archive(member), 'manager')
        member = tarfile.TarInfo('./usr/bin/botstral-manager')
        member.mode = 0o4755
        with self.assertRaisesRegex(ValueError, 'mode'):
            validate_payload(Archive(member), 'manager')


if __name__ == '__main__':
    unittest.main()
