#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess


def ssh(host, arguments, **options):
    return subprocess.run(['ssh', '-oBatchMode=yes', '-oStrictHostKeyChecking=yes', host, shlex.join(arguments)], check=True, **options)


def logged(host, arguments, path, timeout=180):
    with path.open('wb') as output:
        ssh(host, arguments, stdout=output, stderr=subprocess.STDOUT, timeout=timeout)


def fixture(host):
    result = ssh(host, ['python3', '-c', 'import os,tempfile; os.umask(0o077); print(tempfile.mkdtemp(prefix="botstral-m06-packaging-",dir=os.path.expanduser("~")))'], capture_output=True, text=True, timeout=15)
    path = result.stdout.strip()
    if not re.fullmatch(r'/home/[A-Za-z0-9_-]+/botstral-m06-packaging-[A-Za-z0-9_-]+', path):
        raise ValueError('unexpected remote fixture path')
    return path


def main():
    parser = argparse.ArgumentParser(description='Verify native packaging in isolated roots on two existing Linux guests. Existing services are untouched.')
    parser.add_argument('--manager-ssh', default='botstral-manager')
    parser.add_argument('--worker-ssh', default='botstral-computer')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--keep-fixtures', action='store_true')
    args = parser.parse_args()
    if any(not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.-]*', host) for host in (args.manager_ssh, args.worker_ssh)):
        raise ValueError('SSH aliases required')
    os.umask(0o077)
    args.output.mkdir(mode=0o700)
    source = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    archive = subprocess.check_output(['git', 'archive', '--format=tar', source])
    report = {'source': source, 'archive_sha256': hashlib.sha256(archive).hexdigest(), 'ok': False, 'hosts': {}}
    try:
        for component, host in (('manager', args.manager_ssh), ('agent', args.worker_ssh)):
            root = fixture(host)
            report['hosts'][component] = {'alias': host, 'root': root}
            ssh(host, ['tar', '-xf', '-', '-C', root], input=archive, capture_output=True, timeout=120)
            logged(host, ['sh', '-c', 'uname -a; cat /etc/os-release; systemd --version'], args.output / (component + '-host.log'))
            machine = ssh(host, ['cat', '/etc/machine-id'], capture_output=True, timeout=15).stdout
            report['hosts'][component]['machine_sha256'] = hashlib.sha256(machine).hexdigest()
        if report['hosts']['manager']['machine_sha256'] == report['hosts']['agent']['machine_sha256']:
            raise ValueError('manager and Computer must be distinct guests')
        worker = report['hosts']['agent']['root']
        manager = report['hosts']['manager']['root']
        path = ssh(args.worker_ssh, ['python3', '-c', 'import os; print(os.path.expanduser("~/.cargo/bin") + ":/usr/bin:/bin")'], capture_output=True, text=True, timeout=15).stdout.strip()
        prefix = ['env', 'PATH=' + path, 'PYTHONDONTWRITEBYTECODE=1', 'CARGO_TARGET_DIR=' + worker + '/target']
        command = shlex.join([*prefix, 'cargo', 'build', '--locked', '--workspace', '--bins'])
        logged(args.worker_ssh, ['sh', '-c', 'cd ' + shlex.quote(worker) + ' && ' + command], args.output / 'linux-build.log', timeout=1800)
        command = shlex.join([*prefix, 'cargo', 'test', '--locked', '-p', 'botstral-manager', 'offline_restore_reopens', '--', '--nocapture'])
        logged(args.worker_ssh, ['sh', '-c', 'cd ' + shlex.quote(worker) + ' && ' + command], args.output / 'linux-restore.log', timeout=600)
        binary = worker + '/target/debug/botstral-manager'
        content = ssh(args.worker_ssh, ['cat', binary], capture_output=True, timeout=120).stdout
        report['manager_binary_sha256'] = hashlib.sha256(content).hexdigest()
        ssh(args.manager_ssh, ['python3', '-c', 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.write_bytes(sys.stdin.buffer.read()); p.chmod(0o700)', manager + '/manager-binary'], input=content, capture_output=True, timeout=120)
        for component, host, root, executable in (
            ('manager', args.manager_ssh, manager, manager + '/manager-binary'),
            ('agent', args.worker_ssh, worker, binary),
        ):
            arguments = ['env', 'PYTHONDONTWRITEBYTECODE=1', 'BOTSTRAL_MANAGER_BINARY=' + executable, 'python3', '-m', 'unittest', 'discover', '-s', 'manager/packaging', '-p', 'test_*.py', '-v']
            logged(host, ['sh', '-c', 'cd ' + shlex.quote(root) + ' && ' + shlex.join(arguments)], args.output / (component + '-packaging.log'))
            package_binary = executable if component == 'manager' else worker + '/target/debug/botstral-agent'
            logged(host, ['python3', root + '/' + component + '/packaging/install.py', '--binary', package_binary, '--root', root + '/installed'], args.output / (component + '-install.json'))
        uid = ssh(args.worker_ssh, ['id', '-u'], capture_output=True, text=True, timeout=15).stdout.strip()
        logged(args.worker_ssh, ['env', 'XDG_RUNTIME_DIR=/run/user/' + uid, 'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/' + uid + '/bus', worker + '/target/debug/botstral-agent-supervisor-check', '--scratch', worker + '/supervisor-proof'], args.output / 'worker-supervisor.log')
        report['ok'] = True
    finally:
        report['fixtures_retained'] = args.keep_fixtures
        if not args.keep_fixtures:
            for value in report['hosts'].values():
                try:
                    ssh(value['alias'], ['python3', '-c', 'import shutil,sys; shutil.rmtree(sys.argv[1])', value['root']], capture_output=True, timeout=120)
                except subprocess.SubprocessError as error:
                    value['cleanup_error'] = str(error)
                    report['ok'] = False
        (args.output / 'report.json').write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
    print(json.dumps(report, sort_keys=True))


if __name__ == '__main__':
    main()
