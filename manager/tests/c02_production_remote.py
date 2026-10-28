#!/usr/bin/env python3
"""Private guest-account helper for the C02 two-VM fixture."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import m05_remote as legacy

WORKER = '/home/botstral/m04-final-acceptance.GycLnu/bin/production/botstral-agent'
RUNTIME = '/home/botstral/w01-runtime-76b03bf1'
WORKER_HASH = 'a1dffc665e7eb6176a53fe5f325a0874f20f4ea526e9eb23b7a24ba412365a00'
RUNTIME_HASH = '1e3b96245240d969ab380e4ce843f6ac68d27444d425cf211f75e4d2b6859061'


def identities():
    values = {WORKER: WORKER_HASH, RUNTIME+'/runtime.json': RUNTIME_HASH}
    observed = {p: hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in values}
    if values != observed:
        raise RuntimeError('Retained production identity mismatch')
    manifest = json.loads(Path(RUNTIME+'/runtime.json').read_text())
    for name, expected in manifest:
        if hashlib.sha256((Path(RUNTIME)/name).read_bytes()).hexdigest() != expected:
            raise RuntimeError('Retained runtime content mismatch')
    return dict(files=observed, verified_runtime_files=len(manifest))


def dispatch(root, p):
    if not str(root).startswith('/home/botstral/c02-production-'):
        raise ValueError('C02 fixture root')
    op = p['op']
    if op == 'identities':
        return identities()
    if op == 'install_worker':
        identities()
        (root/'bin/botstral-agent').symlink_to(WORKER)
        return True
    if op == 'worker_enroll':
        identities()
        args = [WORKER, 'enroll', '--state-dir', str(root/'agent'), '--manager', 'wss://'+p['address'], '--manager-ca', str(root/'ca.pem'), '--client-cert', str(root/'worker.pem'), '--client-key', str(root/'worker.key'), '--invitation-file', str(root/'worker-invitation.json')]
        return legacy.run(args, timeout=20).strip()
    if op == 'start':
        unit = p['unit']
        if not unit.startswith('botstral-m05-c02-'):
            raise ValueError('C02 unit identity')
        executable = WORKER if p['executable'] == 'installed_worker' else str(root/p['executable'])
        args = ['systemd-run', '--user', '--unit='+unit, '--property=Type=exec', '--property=RemainAfterExit=yes', '--property=UMask=0077', '--property=WorkingDirectory='+str(root), '--property=StandardOutput=append:'+str(root/p['log']), '--property=StandardError=append:'+str(root/p['log'])]
        args += ['--setenv='+key+'='+value for key, value in p.get('env', {}).items()]
        legacy.run([*args, executable, *p['args']])
        return True
    if op == 'stop':
        if not p['unit'].startswith('botstral-m05-c02-'):
            raise ValueError('C02 unit identity')
        result = subprocess.run(['systemctl', '--user', 'show', p['unit'], '--property=LoadState,ActiveState,ExecMainPID'], capture_output=True, text=True)
        state = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
        if state.get('LoadState') == 'not-found' and state.get('ActiveState') == 'inactive' and state.get('ExecMainPID') == '0':
            return dict(already_absent=True, final_unit_state='inactive')
    if op == 'root_processes':
        result = []
        for directory in Path('/proc').iterdir():
            if directory.name.isdigit() and int(directory.name) != os.getpid():
                try:
                    command = (directory/'cmdline').read_bytes()
                    if str(root).encode() in command:
                        result.append(dict(pid=int(directory.name), command=command.decode(errors='replace').replace('\0', ' ')))
                except (FileNotFoundError, PermissionError, ProcessLookupError):
                    pass
        return result
    if op == 'processes':
        result = []
        for pid in p['pids']:
            directory = Path('/proc')/str(int(pid))
            if directory.exists():
                result.append(dict(pid=pid, stat=(directory/'stat').read_text(), cgroup=(directory/'cgroup').read_text()))
        return result
    if op == 'scope':
        unit = p['unit']
        if not unit.startswith('botstral-') or '/' in unit:
            raise ValueError('Worker scope')
        result = subprocess.run(['systemctl', '--user', 'show', unit, '--property=ActiveState,SubState,ControlGroup,MainPID'], capture_output=True, text=True)
        return dict(code=result.returncode, output=result.stdout)
    return legacy.dispatch(root, p)


if __name__ == '__main__':
    os.umask(0o077)
    try:
        result = dispatch(Path(__file__).resolve().parent, json.load(sys.stdin))
        print(json.dumps(dict(ok=True, value=result)))
    except Exception as error:
        print(json.dumps(dict(ok=False, error=type(error).__name__)))
        sys.exit(1)
