#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid


def run(args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=90, **kwargs).stdout


FIXTURE = r'''
import json, os, pathlib, shutil, sqlite3, subprocess, sys
os.umask(0o077)
home = pathlib.Path(os.environ.get('BOTSTRAL_ACCEPTANCE_HOME', '/home/botstral-agent'))
state = home / 'fixture-state'
state.mkdir(mode=0o700)
certs = home / 'fixture-certs'
certs.mkdir(mode=0o700)
(certs / 'fixture.pem').write_text('isolated invalid TLS fixture; no network authentication')
config = {'manager': 'wss://127.0.0.1:1', 'pin': '0' * 64, 'client_cert': str(certs / 'fixture.pem'), 'client_key': str(certs / 'fixture.pem')}
(state / 'config.json').write_text(json.dumps(config))
process = {'profile': 'linux_systemd_v1', 'account': {'uid': os.geteuid(), 'gid': os.getegid()}, 'ceilings': {'runtime_seconds': 10, 'output_bytes': 1024, 'processes': 8, 'memory_bytes': 67108864}}
(state / 'process.json').write_text(json.dumps(process))
runtime = home / 'fixture-runtime'
runtime.mkdir(mode=0o700)
interpreter = runtime / 'python3'
shutil.copyfile(pathlib.Path(sys.executable).resolve(), interpreter)
interpreter.chmod(0o700)
for name, content in CONNECTOR_FILES.items():
    (runtime / name).write_text(content)
configuration = subprocess.check_output([sys.executable, str(runtime / 'configure.py'), '--python', str(interpreter)])
(state / 'connector.json').write_bytes(configuration)
result = subprocess.run(['/usr/bin/botstral-agent', 'check', '--state-dir', str(state)], capture_output=True)
assert result.returncode == 1, 'invalid TLS fixture unexpectedly connected'
assert b'InvalidMessage' not in result.stderr and b'connector runtime changed' not in result.stderr, result.stderr
with sqlite3.connect(state / 'agent.sqlite3') as db:
    assert db.execute('PRAGMA user_version').fetchone()[0] == WORKER_SCHEMA
    identity = db.execute('SELECT incarnation FROM metadata').fetchone()[0]
if os.environ.get('BOTSTRAL_ACCEPTANCE_PACKAGE_ONLY') == '1':
    sys.exit(0)
plan = home / 'fixture-plan.json'
roots = [{'label': 'certificates', 'kind': 'certificate', 'path': str(certs)}, {'label': 'connector-runtime', 'kind': 'runtime', 'path': str(runtime)}]
plan.write_text(json.dumps({'roots': roots}))
diagnostic = ['/usr/bin/botstral-agent-diagnose', '--state-dir', str(state), '--plan', str(plan)]
assert json.loads(subprocess.check_output(diagnostic))['runtime_hashes'] == ['connector.json']
original = interpreter.read_bytes()
interpreter.write_bytes(b'tampered interpreter')
refused = subprocess.run(diagnostic, capture_output=True, text=True)
assert refused.returncode == 1 and 'connector interpreter digest mismatch' in refused.stderr, refused.stderr
interpreter.write_bytes(original)
omitted = home / 'omitted-runtime-plan.json'
omitted.write_text(json.dumps({'roots': roots[:1]}))
refused = subprocess.run(['/usr/bin/botstral-agent-backup', 'create', '--state-dir', str(state), '--plan', str(omitted), '--destination', str(home / 'incomplete-backup')], capture_output=True, text=True)
assert refused.returncode == 1 and 'runtime reference is not covered' in refused.stderr, refused.stderr
backup = home / 'fixture-backup'
holder = subprocess.Popen(['/usr/bin/sleep', '30'], cwd=certs)
try:
    refused = subprocess.run(['/usr/bin/botstral-agent-backup', 'create', '--state-dir', str(state), '--plan', str(plan), '--destination', str(backup)], capture_output=True, text=True)
    assert refused.returncode == 1 and 'process still holds a selected root' in refused.stderr, refused.stderr
finally:
    holder.terminate()
    holder.wait(timeout=5)
subprocess.run(['/usr/bin/botstral-agent-backup', 'create', '--state-dir', str(state), '--plan', str(plan), '--destination', str(backup)], check=True)
subprocess.run(['/usr/bin/botstral-agent-backup', 'verify', '--backup', str(backup)], check=True)
runtime.rename(home / 'retired-runtime')
certs.rename(home / 'retired-certificates')
state.rename(home / 'retired-state')
restored = state
subprocess.run(['/usr/bin/botstral-agent-backup', 'restore', '--backup', str(backup), '--destination', str(restored), '--original-instance-retired'], check=True)
with sqlite3.connect(restored / 'agent.sqlite3') as db:
    assert db.execute('SELECT incarnation FROM metadata').fetchone()[0] == identity
assert (certs / 'fixture.pem').read_text() == 'isolated invalid TLS fixture; no network authentication'
assert interpreter.read_bytes() == original
result = subprocess.run(['/usr/bin/botstral-agent', 'check', '--state-dir', str(state)], capture_output=True)
assert result.returncode == 1 and b'InvalidMessage' not in result.stderr and b'connector runtime changed' not in result.stderr, result.stderr
assert json.loads(subprocess.check_output(diagnostic))['runtime_hashes'] == ['connector.json']
print(json.dumps({'production_journal_identity_preserved': True, 'selected_certificate_bytes_restored': True, 'connector_interpreter_restored': True, 'connector_tampering_refused': True, 'omitted_connector_runtime_refused': True}))
'''


def main():
    parser = argparse.ArgumentParser(description='Requires a dedicated Linux Computer host. This starts a privileged disposable container.')
    parser.add_argument('--engine', choices=['docker', 'podman'], required=True)
    parser.add_argument('--image', required=True)
    parser.add_argument('--evidence', required=True)
    args = parser.parse_args()
    name = 'botstral-m06-proof-' + uuid.uuid4().hex[:10]
    base = [args.engine, 'run', '--detach', '--name', name, '--privileged', '--cgroupns=host', '--stop-signal=SIGRTMIN+3', '--tmpfs', '/run', '--tmpfs', '/run/lock', '--tmpfs', '/tmp', '--mount', 'type=bind,src=/sys/fs/cgroup,dst=/sys/fs/cgroup']
    if args.engine == 'podman':
        base.append('--systemd=always')
    result = {'image': args.image, 'engine': args.engine, 'ok': False}
    try:
        result['container'] = run([*base, args.image]).strip()
        deadline = time.monotonic() + 30
        while True:
            ready = subprocess.run([args.engine, 'exec', name, 'systemctl', 'is-active', 'user@10001.service'], capture_output=True, text=True, timeout=5)
            if ready.returncode == 0:
                break
            if time.monotonic() >= deadline:
                raise ValueError('systemd user manager did not start')
            time.sleep(.2)
        user = [args.engine, 'exec', '--user', '10001:10001', name, 'env', 'XDG_RUNTIME_DIR=/run/user/10001', 'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/10001/bus']
        run([*user, 'systemctl', '--user', 'stop', 'botstral-agent.service'])
        result['supervisor'] = run([*user, '/usr/bin/botstral-agent-supervisor-check', '--scratch', '/home/botstral-agent/supervisor-proof']).strip()
        connector = Path(__file__).resolve().parents[1] / 'connector'
        files = {name: (connector / name).read_text() for name in ('configure.py', 'runtime.py')}
        schema = json.loads((Path(__file__).resolve().parents[2] / 'release.json').read_text())['data_compatibility']['worker_schema']
        fixture = 'CONNECTOR_FILES = ' + repr(files) + '\nWORKER_SCHEMA = ' + str(schema) + '\n' + FIXTURE
        result['recovery'] = run([*user, 'python3', '-c', fixture]).strip()
        result['packages'] = run([args.engine, 'exec', name, 'cat', '/usr/share/botstral-image-packages.txt'])
        result['ok'] = True
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        result['error'] = str(error)
        if isinstance(error, subprocess.CalledProcessError):
            result['stderr'] = error.stderr
        result['container_logs'] = subprocess.run([args.engine, 'logs', name], capture_output=True, text=True, timeout=10).stdout
        raise
    finally:
        subprocess.run([args.engine, 'rm', '--force', name], capture_output=True, timeout=30)
        os.umask(0o077)
        with open(args.evidence, 'x') as stream:
            json.dump(result, stream, sort_keys=True, indent=2)
            stream.write('\n')


if __name__ == '__main__':
    main()
