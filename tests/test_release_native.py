import json
import os
from pathlib import Path
import plistlib
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from release_model import MARKER, ReleaseIdentity


class NativeValidationDiagnosticsTests(unittest.TestCase):
    def test_failed_app_checks_retain_external_process_diagnostics(self):
        identity = ReleaseIdentity.parse({'version': '0.1.0', 'channel': 'stable',
                                          'revision': 'a' * 40, 'build_id': 'synthetic-diagnostics'})
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / 'Synthetic.app'
            (app / 'Contents/MacOS').mkdir(parents=True)
            (app / 'Contents/Info.plist').write_bytes(plistlib.dumps({
                'CFBundleIdentifier': 'com.botstral.desktop',
                'CFBundleVersion': identity.native_versions()['macos_bundle'],
                'CFBundleShortVersionString': identity.native_versions()['macos_short']}))
            executable = app / 'Contents/MacOS/botstral-desktop'
            executable.write_bytes(b'\xcf\xfa\xed\xfe' + struct.pack('<I', 0x0100000c)
                                   + MARKER + json.dumps({'identity': identity.json()}).encode() + b'\0')
            tool = root / 'synthetic-validator'
            tool.write_text('#!' + sys.executable + '\n' + '''import os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
operation = name + ('-detail' if '-d' in sys.argv else '')
if operation == os.environ.get('SYNTHETIC_FAIL'):
    os.write(1, b'synthetic validator stdout\\n')
    os.write(2, b'synthetic validator stderr\\n')
    sys.exit(68)
if operation == 'codesign-detail':
    print('Authority=Developer ID Application: Synthetic fixture', file=sys.stderr)
    print('TeamIdentifier=SYNTHETIC', file=sys.stderr)
''')
            tool.chmod(0o755)
            for name in ('codesign', 'spctl', 'xcrun'):
                (root / name).symlink_to(tool)
            code = ('from release_artifacts import inspect_app; from release_model import ReleaseIdentity; '
                    'import json, sys; result = inspect_app(sys.argv[1], ReleaseIdentity.parse(json.loads(sys.argv[2])), True); '
                    'assert str(result) == sys.argv[3]')
            environment = dict(os.environ, PATH=str(root), PYTHONPATH=str(Path(__file__).resolve().parents[1] / 'scripts'))
            for failure in ('codesign', 'codesign-detail', 'spctl', 'xcrun', ''):
                with self.subTest(failure=failure):
                    result = subprocess.run([sys.executable, '-c', code, str(app), json.dumps(identity.json()), str(executable)],
                                            env=dict(environment, SYNTHETIC_FAIL=failure), capture_output=True)
                    if failure:
                        self.assertNotEqual(result.returncode, 0)
                        self.assertIn(b'exit status 68', result.stderr)
                        self.assertEqual(result.stdout, b'synthetic validator stdout\n')
                        self.assertIn(b'synthetic validator stderr\n', result.stderr)
                    else:
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertEqual((result.stdout, result.stderr), (b'', b''))


@unittest.skipUnless(sys.platform == 'linux' and os.geteuid() == 0,
                     'requires root on disposable Linux for a distinct fixture UID')
class LinuxFixtureExecutionTests(unittest.TestCase):
    def test_helper_executes_after_demotion_without_exposing_cargo_tree(self):
        from release_acceptance_linux import fixture_binary
        def demote():
            os.setgroups([])
            os.setgid(65534)
            os.setuid(65534)
        with tempfile.TemporaryDirectory(prefix='synthetic-helper-') as directory:
            root = Path(directory)
            root.chmod(0o755)
            source = root / 'private-cargo-tree'
            source.mkdir(mode=0o700)
            binary = source / 'synthetic-manager-test'
            shutil.copyfile('/bin/echo', binary)
            binary.chmod(0o700)
            stage = root / 'disposable-fixture'
            stage.mkdir(mode=0o755)
            original = (source.stat().st_mode, binary.stat().st_mode, binary.read_bytes())
            with self.assertRaises(PermissionError):
                subprocess.run([str(binary), 'synthetic fixture'], preexec_fn=demote, check=True)
            output = json.dumps({'reason': 'compiler-artifact', 'executable': str(binary),
                                 'target': {'name': 'botstral_manager'}})
            previous_umask = os.umask(0o077)
            try:
                with patch('release_acceptance_linux.subprocess.check_output', return_value=output):
                    helper = fixture_binary(stage)
            finally:
                os.umask(previous_umask)
            result = subprocess.run([str(helper), 'synthetic fixture'], preexec_fn=demote,
                                    check=True, capture_output=True, text=True)
            self.assertEqual(result.stdout, 'synthetic fixture\n')
            self.assertEqual(helper.parent, stage)
            self.assertEqual(stat.S_IMODE(helper.stat().st_mode), 0o755)
            self.assertEqual(helper.stat().st_uid, 0)
            self.assertEqual((source.stat().st_mode, binary.stat().st_mode, binary.read_bytes()), original)


if __name__ == '__main__':
    unittest.main()
