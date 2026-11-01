#!/usr/bin/env python3
import argparse
import json
import subprocess
import sys

FIXTURE = r'''
import json, os, pathlib, signal, socket, subprocess, tempfile, time
os.umask(0o077)
with tempfile.TemporaryDirectory() as directory:
    root = pathlib.Path(directory)
    state = root / 'state'
    state.mkdir(mode=0o700)
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(root / 'server.key'), '-out', str(root / 'server.pem'), '-days', '1', '-subj', '/CN=localhost'], check=True, capture_output=True)
    (root / 'server.pem').chmod(0o600)
    held = [socket.socket() for _ in range(3)]
    for selected in held:
        selected.bind(('127.0.0.1', 0))
    addresses = ['127.0.0.1:' + str(selected.getsockname()[1]) for selected in held]
    for selected in held:
        selected.close()
    config = {'state_dir': str(state), 'listen': addresses[0], 'setup_listen': addresses[1], 'public_listen': addresses[2]}
    path = root / 'service.json'
    path.write_text(json.dumps(config))
    process = subprocess.Popen(['/usr/lib/botstral-manager/launch.py', '--config', str(path)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + 10
        while not (state / 'listeners.json').exists():
            assert process.poll() is None, process.stderr.read().decode()
            assert time.monotonic() < deadline, 'manager startup deadline'
            time.sleep(.02)
        listeners = json.loads((state / 'listeners.json').read_text())
        assert listeners['listen'] == addresses[0]
        print(json.dumps({'nonroot_uid': os.geteuid(), 'file_configuration_and_private_tls': True, 'manager_started': True}))
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
        assert process.wait(timeout=10) == 0
        process.stderr.close()
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--engine', choices=['docker', 'podman'], required=True)
    parser.add_argument('--image', required=True)
    args = parser.parse_args()
    result = subprocess.run([args.engine, 'run', '--rm', '--read-only', '--tmpfs', '/tmp:mode=1777', '--cap-drop=ALL', '--security-opt=no-new-privileges', '--entrypoint', '/usr/bin/python3', args.image, '-c', FIXTURE], check=True, capture_output=True, text=True, timeout=30)
    print(result.stdout.strip())


if __name__ == '__main__':
    main()
