import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import unittest

import install
import launch
import diagnose

ROOT = Path(__file__).resolve().parents[2]


class InstallationTests(unittest.TestCase):
    def test_manager_and_worker_cannot_share_an_installation_root(self):
        for first, second in [('manager', 'agent'), ('agent', 'manager')]:
            with self.subTest(first=first), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                binary = root / 'binary'
                binary.write_text('#!/bin/sh\nexit 0\n')
                binary.with_name('botstral-agent-supervisor-check').write_text('#!/bin/sh\nexit 0\n')
                destination = root / 'staged'
                destination.mkdir(mode=0o700)
                install.install(first, binary, destination, ROOT / first / 'packaging')
                with self.assertRaisesRegex(ValueError, 'separate installation roots'):
                    install.install(second, binary, destination, ROOT / second / 'packaging')
                self.assertFalse((destination / ('usr/lib/botstral-' + second)).exists())

    def test_connector_interpreter_is_verified_and_tampering_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            interpreter = root / 'python'
            interpreter.write_bytes(b'pinned connector interpreter')
            (root / 'connector.json').write_text(json.dumps({'python': str(interpreter), 'python_sha256': diagnose.digest(interpreter)}))
            self.assertEqual(diagnose.runtime_hashes(root), ['connector.json'])
            interpreter.write_bytes(b'changed interpreter')
            with self.assertRaisesRegex(ValueError, 'connector interpreter digest mismatch'):
                diagnose.runtime_hashes(root)
            interpreter.unlink()
            with self.assertRaises(FileNotFoundError):
                diagnose.runtime_hashes(root)

    def test_sealed_vibe_runtime_is_accepted_and_tampering_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            runtime = root / 'vibe'
            runtime.mkdir(mode=0o700)
            executable = runtime / 'vibe.py'
            executable.write_text('print("pinned runtime")\n')
            manifest = root / 'vibe-manifest.json'
            manifest.write_text(json.dumps({'version': '2.24.2', 'distributions': {'mistral-vibe': '2.24.2'}, 'files': [['vibe.py', diagnose.digest(executable)]]}))
            (root / 'vibe.json').write_text(json.dumps({'directory': str(runtime), 'manifest': str(manifest), 'runtime': diagnose.digest(manifest)}))
            self.assertEqual(diagnose.runtime_hashes(root), ['vibe.json'])
            executable.write_text('modified runtime\n')
            with self.assertRaisesRegex(ValueError, 'digest mismatch'):
                diagnose.runtime_hashes(root)

    def test_separate_install_layout_and_refusal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            binary = root / 'binary'
            binary.write_text('#!/bin/sh\nexit 0\n')
            binary.with_name('botstral-agent-supervisor-check').write_text('#!/bin/sh\nexit 0\n')
            for component in ('manager', 'agent'):
                destination = root / component
                destination.mkdir(mode=0o700)
                manifest = install.install(component, binary, destination, ROOT / component / 'packaging')
                self.assertFalse(manifest['services_enabled'])
                library = destination / ('usr/lib/botstral-' + component)
                help_result = subprocess.run(['python3', str(library / 'backup.py'), '--help'], capture_output=True, text=True, check=True)
                self.assertIn('restore', help_result.stdout)
                self.assertEqual((destination / ('usr/bin/botstral-' + component)).stat().st_mode & 0o777, 0o755)
                unit = destination / ('usr/lib/systemd/' + ('system' if component == 'manager' else 'user')) / ('botstral-' + component + '.service')
                self.assertTrue(unit.is_file())
                self.assertFalse((destination / 'usr/lib/systemd' / ('user' if component == 'manager' else 'system') / unit.name).exists())
                with self.assertRaisesRegex(ValueError, 'already installed'):
                    install.install(component, binary, destination, ROOT / component / 'packaging')

    def test_staging_symlink_cannot_escape_install_root(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            binary = root / 'binary'
            binary.write_text('fixture binary')
            destination = root / 'staged'
            (destination / 'usr').mkdir(parents=True)
            outside = root / 'outside'
            outside.mkdir()
            (destination / 'usr/bin').symlink_to(outside)
            with self.assertRaisesRegex(ValueError, 'symlink'):
                install.install('manager', binary, destination)
            self.assertEqual(list(outside.iterdir()), [])
            destination = root / 'second-stage'
            (destination / 'usr/bin').mkdir(parents=True)
            (destination / 'usr/bin/botstral-manager-diagnose').symlink_to(outside / 'diagnose')
            with self.assertRaisesRegex(ValueError, 'diagnostic command already exists'):
                install.install('manager', binary, destination)
            self.assertEqual(list(outside.iterdir()), [])

    def test_real_manager_uses_file_configuration_and_private_tls(self):
        manager = Path(os.environ.get('BOTSTRAL_MANAGER_BINARY', ROOT / 'target/debug/botstral-manager')).resolve()
        self.assertTrue(manager.is_file(), 'build the manager before running installation tests')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            root.chmod(0o700)
            state = root / 'state'
            state.mkdir(mode=0o700)
            subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(root / 'server.key'), '-out', str(root / 'server.pem'), '-days', '1', '-subj', '/CN=localhost'], check=True, capture_output=True)
            for path in (root / 'server.key', root / 'server.pem'):
                path.chmod(0o600)
            sockets = [socket.socket() for _ in range(3)]
            for selected in sockets:
                selected.bind(('127.0.0.1', 0))
            addresses = ['127.0.0.1:' + str(selected.getsockname()[1]) for selected in sockets]
            for selected in sockets:
                selected.close()
            configuration = {'state_dir': str(state), 'listen': addresses[0], 'setup_listen': addresses[1], 'public_listen': addresses[2], 'certificate': str(root / 'server.pem'), 'key': str(root / 'server.key'), 'worker_ca': str(root / 'server.pem')}
            path = root / 'service.json'
            path.write_text(json.dumps(configuration))
            path.chmod(0o600)
            command = launch.arguments(path, str(manager))
            self.assertNotIn((root / 'server.key').read_text(), ' '.join(command))
            process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            try:
                deadline = time.monotonic() + 10
                while not (state / 'listeners.json').exists():
                    self.assertIsNone(process.poll(), process.stderr.read().decode() if process.poll() is not None else '')
                    self.assertLess(time.monotonic(), deadline, 'manager startup deadline')
                    time.sleep(.02)
                self.assertEqual(json.loads((state / 'listeners.json').read_text())['listen'], addresses[0])
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                self.assertEqual(process.wait(timeout=10), 0)
                process.stderr.close()
            (root / 'server.key').chmod(0o644)
            with self.assertRaisesRegex(ValueError, 'private permissions'):
                launch.arguments(path, str(manager))
            (root / 'server.key').chmod(0o600)
            configuration['listen'] = '0.0.0.0:43120'
            path.write_text(json.dumps(configuration))
            with self.assertRaisesRegex(ValueError, 'listener'):
                launch.arguments(path, str(manager))


if __name__ == '__main__':
    os.umask(0o077)
    unittest.main()
