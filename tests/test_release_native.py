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


class StaplerTransportTests(unittest.TestCase):
    transport_diagnostic = (
        'error is Error Domain=NSURLErrorDomain Code=-1005 "The network connection was lost." '
        'UserInfo={NSLocalizedDescription=The network connection was lost., '
        'NSErrorFailingURLStringKey=https://api.apple-cloudkit.com/database/1/'
        'com.apple.gk.ticket-delivery/production/public/records/lookup, '
        'NSErrorFailingURLKey=https://api.apple-cloudkit.com/database/1/'
        'com.apple.gk.ticket-delivery/production/public/records/lookup}\n'
        "CloudKit's response is inconsistent with expections: (null)\n"
        'The validate action failed! Error 68.\n')
    nightly_staple_diagnostic = (
        'error is Error Domain=NSURLErrorDomain Code=-1005 "The network connection was lost." '
        'UserInfo={_kCFStreamErrorCodeKey=-4, NSUnderlyingError=0x600001c29e00 '
        '{Error Domain=kCFErrorDomainCFNetwork Code=-1005 "(null)" '
        'UserInfo={NSErrorPeerAddressKey=<CFData 0x60000312d4f0 [0x205123fa8]>'
        '{length = 16, capacity = 16, bytes = 0x100201bb11fa61220000000000000000}, '
        '_kCFStreamErrorCodeKey=-4, _kCFStreamErrorDomainKey=4}}, '
        '_NSURLErrorFailingURLSessionTaskErrorKey=LocalDataTask '
        '<596751C1-84B9-4F02-AB0E-072CAEAC43D2>.<1>, '
        '_NSURLErrorRelatedURLSessionTaskErrorKey=(\n'
        '    "LocalDataTask <596751C1-84B9-4F02-AB0E-072CAEAC43D2>.<1>"\n'
        '), NSLocalizedDescription=The network connection was lost., '
        'NSErrorFailingURLStringKey=https://api.apple-cloudkit.com/database/1/'
        'com.apple.gk.ticket-delivery/production/public/records/lookup, '
        'NSErrorFailingURLKey=https://api.apple-cloudkit.com/database/1/'
        'com.apple.gk.ticket-delivery/production/public/records/lookup, '
        '_kCFStreamErrorDomainKey=4}\n'
        "CloudKit's response is inconsistent with expections: (null)\n"
        'The staple and validate action failed! Error 68.\n')

    def run_validator(self, command, failures, *, diagnostic=None, returncode=68, text=False,
                      diagnostic_stream='stderr', entry='checked_command'):
        with tempfile.TemporaryDirectory(prefix='synthetic-ticket-') as directory:
            root = Path(directory)
            (root / 'config.json').write_text(json.dumps({
                'failures': failures, 'returncode': returncode, 'diagnostic_stream': diagnostic_stream,
                'diagnostic': self.transport_diagnostic if diagnostic is None else diagnostic}))
            tool = root / 'synthetic-validator'
            tool.write_text('#!' + sys.executable + '\n' + '''import json, pathlib, sys
root = pathlib.Path(__file__).resolve().parent
config = json.loads((root / 'config.json').read_text())
with (root / 'attempts').open('a') as attempts:
    attempts.write(json.dumps(sys.argv[1:]) + '\\n')
attempt = len((root / 'attempts').read_text().splitlines())
if attempt <= config['failures']:
    print(f'synthetic failed inspection {attempt}', flush=True)
    print(config['diagnostic'], end='', file=getattr(sys, config['diagnostic_stream']), flush=True)
    sys.exit(config['returncode'])
print('validated')
print('validation detail', file=sys.stderr)
''')
            tool.chmod(0o755)
            for name in ('xcrun', 'codesign', 'spctl', 'hdiutil', 'installer'):
                (root / name).symlink_to(tool)
            code = '''import json, pathlib, subprocess, sys
if sys.argv[4] == 'package_command':
    from release_package import command as run
else:
    from release_artifacts import checked_command as run
text = json.loads(sys.argv[2])
try:
    result = run(json.loads(sys.argv[1]), text=text)
except subprocess.CalledProcessError as error:
    def decode(value):
        if value is None:
            return ''
        return value if isinstance(value, str) else value.decode()
    pathlib.Path(sys.argv[3]).write_text(json.dumps({
        'args': error.cmd, 'returncode': error.returncode,
        'stdout': decode(error.stdout), 'stderr': decode(error.stderr),
        'text': isinstance(error.stdout, str)}))
    raise
assert result.stdout == ('validated\\n' if text else b'validated\\n'), result.stdout
assert result.stderr == ('validation detail\\n' if text else b'validation detail\\n'), result.stderr
'''
            error_file = root / 'error.json'
            result = subprocess.run([sys.executable, '-c', code, json.dumps(command),
                                     json.dumps(text), str(error_file), entry], capture_output=True, timeout=10,
                                    env=dict(os.environ, PATH=str(root),
                                             PYTHONPATH=str(Path(__file__).resolve().parents[1] / 'scripts')))
            attempts = [json.loads(line) for line in (root / 'attempts').read_text().splitlines()]
            error = json.loads(error_file.read_text()) if error_file.exists() else None
            return result, attempts, error

    def test_connection_loss_then_actual_validation_success(self):
        for text in (False, True):
            with self.subTest(text=text):
                result, attempts, error = self.run_validator(
                    ['xcrun', 'stapler', 'validate', 'Synthetic.app'], 1, text=text)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertEqual(attempts, [['stapler', 'validate', 'Synthetic.app']] * 2)
                self.assertIsNone(error)
                self.assertEqual(result.stdout, b'')
                self.assertIn(b'retrying stapler validate', result.stderr)

    def test_normal_success_is_quiet_and_preserves_captured_stream_types(self):
        for text in (False, True):
            with self.subTest(text=text):
                result, attempts, error = self.run_validator(
                    ['xcrun', 'stapler', 'validate', 'Synthetic.app'], 0, text=text)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertEqual(len(attempts), 1)
                self.assertIsNone(error)
                self.assertEqual((result.stdout, result.stderr), (b'', b''))

    def test_permanent_invalid_ticket_fails_once_with_diagnostics(self):
        diagnostic = 'The ticket is invalid.\nThe validate action failed! Error 68.\n'
        result, attempts, error = self.run_validator(
            ['xcrun', 'stapler', 'validate', 'Synthetic.app'], 10, diagnostic=diagnostic)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(attempts), 1)
        self.assertEqual(error['returncode'], 68)
        self.assertEqual(result.stdout, b'synthetic failed inspection 1\n')
        self.assertIn(diagnostic.encode(), result.stderr)
        self.assertNotIn(b'retrying', result.stderr)

    def test_other_operations_never_retry_transport_diagnostic(self):
        commands = [
            ['codesign', '--verify', 'Synthetic.app'],
            ['spctl', '--assess', 'Synthetic.app'],
            ['hdiutil', 'attach', 'Synthetic.dmg'],
            ['hdiutil', 'detach', 'volume'],
            ['installer', '-pkg', 'Synthetic.pkg'],
            ['xcrun', 'notarytool', 'submit', 'Synthetic.app'],
            ['xcrun', 'stapler', 'validate', 'Synthetic.app', '--unknown'],
        ]
        for command in commands:
            with self.subTest(command=command):
                result, attempts, error = self.run_validator(command, 10)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(len(attempts), 1)
                self.assertEqual(error['args'], command)
                self.assertNotIn(b'retrying', result.stderr)

    def test_only_observed_transport_diagnostic_and_exit_code_retry(self):
        diagnostics = [
            '',
            self.transport_diagnostic.replace('Code=-1005', 'Code=-1001'),
            self.transport_diagnostic.replace('NSURLErrorDomain', 'OtherErrorDomain'),
            self.transport_diagnostic.replace('The network connection was lost.', 'Invalid ticket.'),
            self.transport_diagnostic.replace('api.apple-cloudkit.com', 'example.invalid'),
            self.transport_diagnostic.replace('/records/lookup,', '/records/lookup/other,'),
            self.transport_diagnostic.replace('NSErrorFailingURLStringKey=', 'OtherKey='),
        ]
        for diagnostic, returncode in [(value, 68) for value in diagnostics] + [(self.transport_diagnostic, 1)]:
            with self.subTest(diagnostic=diagnostic, returncode=returncode):
                result, attempts, error = self.run_validator(
                    ['xcrun', 'stapler', 'validate', 'Synthetic.app'], 10,
                    diagnostic=diagnostic, returncode=returncode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(len(attempts), 1)
                self.assertEqual(error['returncode'], returncode)
                self.assertNotIn(b'retrying', result.stderr)

    def test_transport_exhaustion_rethrows_final_failure_with_diagnostics(self):
        command = ['xcrun', 'stapler', 'validate', 'Synthetic.app']
        for text in (False, True):
            for diagnostic_stream in ('stdout', 'stderr'):
                with self.subTest(text=text, diagnostic_stream=diagnostic_stream):
                    result, attempts, error = self.run_validator(
                        command, 10, text=text, diagnostic_stream=diagnostic_stream)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(attempts, [command[1:]] * 3)
                    expected_stdout = 'synthetic failed inspection 3\n'
                    expected_stderr = ''
                    if diagnostic_stream == 'stdout':
                        expected_stdout += self.transport_diagnostic
                    else:
                        expected_stderr = self.transport_diagnostic
                    self.assertEqual(error, {'args': command, 'returncode': 68, 'text': text,
                                             'stdout': expected_stdout, 'stderr': expected_stderr})
                    self.assertEqual(result.stdout, expected_stdout.encode())
                    self.assertIn(self.transport_diagnostic.encode(), result.stdout + result.stderr)
                    self.assertIn(b'exit status 68', result.stderr)
                    self.assertEqual(result.stderr.count(b'retrying stapler validate'), 2)

    def test_nightly_staple_cloudkit_log_retries_then_succeeds(self):
        for entry in ('checked_command', 'package_command'):
            with self.subTest(entry=entry):
                result, attempts, error = self.run_validator(
                    ['xcrun', 'stapler', 'staple',
                     'botstral-desktop_0.1.0-nightly.20261031.4121093217302_arm64.dmg'],
                    1, diagnostic=self.nightly_staple_diagnostic, entry=entry)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertEqual(attempts, [[
                    'stapler', 'staple',
                    'botstral-desktop_0.1.0-nightly.20261031.4121093217302_arm64.dmg']] * 2)
                self.assertIsNone(error)
                self.assertIn(b'retrying stapler staple', result.stderr)

    def test_nightly_staple_cloudkit_log_does_not_retry_unrelated_failures(self):
        command = ['xcrun', 'stapler', 'staple',
                   'botstral-desktop_0.1.0-nightly.20261031.4121093217302_arm64.dmg']
        cases = [
            (self.nightly_staple_diagnostic.replace('Code=-1005', 'Code=-1001'), 68),
            (self.nightly_staple_diagnostic.replace('NSURLErrorDomain', 'OtherErrorDomain'), 68),
            ('The ticket is invalid.\nThe staple and validate action failed! Error 68.\n', 68),
            (self.nightly_staple_diagnostic, 1),
        ]
        for diagnostic, returncode in cases:
            for entry in ('checked_command', 'package_command'):
                with self.subTest(entry=entry, returncode=returncode, diagnostic=diagnostic[:40]):
                    result, attempts, error = self.run_validator(
                        command, 10, diagnostic=diagnostic, returncode=returncode, entry=entry)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(len(attempts), 1)
                    self.assertEqual(error['returncode'], returncode)
                    self.assertNotIn(b'retrying', result.stderr)

    def test_validation_can_succeed_on_last_allowed_attempt(self):
        result, attempts, error = self.run_validator(
            ['xcrun', 'stapler', 'validate', 'Synthetic.app'], 2, diagnostic_stream='stdout')
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(len(attempts), 3)
        self.assertIsNone(error)
        self.assertEqual(result.stdout, b'')
        self.assertEqual(result.stderr.count(b'retrying stapler validate'), 2)


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
