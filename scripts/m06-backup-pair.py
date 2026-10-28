#!/usr/bin/env python3
"""Stop a two-host installation and publish both offline recovery identities."""
import argparse
import datetime
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys


def remote(host, command):
    return subprocess.run(['ssh', '-oBatchMode=yes', host, shlex.join(command)], check=True, capture_output=True, text=True, timeout=180).stdout


def user_command(host, account, command):
    uid = remote(host, ['id', '-u', account]).strip()
    if not uid.isdecimal():
        raise ValueError('invalid account identity')
    return ['sudo', '-n', '-u', account, 'env', 'XDG_RUNTIME_DIR=/run/user/' + uid, 'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/' + uid + '/bus', *command]


def main():
    parser = argparse.ArgumentParser(description='Run only against the installation you intend to stop. Failure leaves services stopped.')
    parser.add_argument('--manager-host', default='botstral-manager')
    parser.add_argument('--worker-host', default='botstral-computer')
    parser.add_argument('--manager-user', default='botstral-manager')
    parser.add_argument('--worker-user', default='botstral-agent')
    parser.add_argument('--manager-state', default='/var/lib/botstral-manager')
    parser.add_argument('--worker-state', default='/home/botstral-agent/.local/share/botstral-agent')
    parser.add_argument('--worker-plan', required=True)
    parser.add_argument('--manager-destination', required=True)
    parser.add_argument('--worker-destination', required=True)
    parser.add_argument('--receipt', required=True)
    parser.add_argument('--restart-after-success', action='store_true')
    args = parser.parse_args()
    os.umask(0o077)
    receipt = Path(args.receipt)
    if receipt.exists():
        raise ValueError('receipt must be new')
    worker_service = user_command(args.worker_host, args.worker_user, ['systemctl', '--user'])
    remote(args.manager_host, ['sudo', '-n', 'systemctl', 'stop', 'botstral-manager.service'])
    remote(args.worker_host, [*worker_service, 'stop', 'botstral-agent.service'])
    stopped_at = datetime.datetime.now(datetime.timezone.utc).isoformat()
    manager = json.loads(remote(args.manager_host, ['sudo', '-n', '-u', args.manager_user, 'botstral-manager-backup', 'create', '--state-dir', args.manager_state, '--destination', args.manager_destination]))
    worker = json.loads(remote(args.worker_host, user_command(args.worker_host, args.worker_user, ['botstral-agent-backup', 'create', '--state-dir', args.worker_state, '--plan', args.worker_plan, '--destination', args.worker_destination])))
    for component, host, account, backup in [('manager', args.manager_host, args.manager_user, args.manager_destination), ('agent', args.worker_host, args.worker_user, args.worker_destination)]:
        remote(host, ['sudo', '-n', '-u', account, 'botstral-' + component + '-backup', 'verify', '--backup', backup])
    result = {'stopped_at': stopped_at, 'manager': manager, 'worker': worker, 'remote_effects': 'shutdown may leave uncertain effects; neither bundle snapshots external services'}
    with open(receipt, 'x', opener=lambda path, flags: os.open(path, flags, 0o600)) as stream:
        json.dump(result, stream, sort_keys=True, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    if args.restart_after_success:
        remote(args.manager_host, ['sudo', '-n', 'systemctl', 'start', 'botstral-manager.service'])
        remote(args.worker_host, [*worker_service, 'start', 'botstral-agent.service'])
    print(json.dumps({'receipt': str(receipt), 'restarted': args.restart_after_success, 'ok': True}))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print('coordinated backup failed; inspect both service states before restart: ' + str(error), file=sys.stderr)
        sys.exit(1)
