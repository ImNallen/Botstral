#!/usr/bin/env python3
"""Idle-gated update of one signed release. The outcome is one tagged result and one report file."""
from __future__ import annotations

import argparse
import dataclasses
from enum import Enum
import fcntl
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import sqlite3
import subprocess
import sys
import time
import urllib.request

HERE = Path(__file__).resolve().parent
TOOLS = HERE / 'tools' / 'scripts'
sys.path.insert(0, str(TOOLS if TOOLS.exists() else HERE))

from release_model import ReleaseIdentity, digest, read_json, require, verify, write_json
from release_upgrade import replacement_policy

CHANNEL = 'https://github.com/ImNallen/Botstral/releases/download/channel-nightly/channel.json'
BUSY = ('active_task', 'active_process')
REJECT = ('signature', 'tamper', 'channel', 'downgrade', 'identity')
SAFE_NAME = re.compile(r'^(manifest\.(json|sig)|botstral-(manager|agent|desktop)_[A-Za-z0-9.+~-]+_(amd64\.deb|arm64\.dmg))$')


class Probe(str, Enum):
    OK = 'ok'
    FAIL = 'fail'


@dataclasses.dataclass(frozen=True)
class Idle:
    def json(self):
        return {'state': 'idle'}


@dataclasses.dataclass(frozen=True)
class Busy:
    reason: str

    def __post_init__(self):
        require(self.reason in BUSY, 'invalid busy reason')

    def json(self):
        return {'state': 'busy', 'reason': self.reason}


@dataclasses.dataclass(frozen=True)
class Ready:
    pass


@dataclasses.dataclass(frozen=True)
class Current:
    def json(self):
        return {'type': 'current'}


@dataclasses.dataclass(frozen=True)
class Deferred:
    busy: Busy

    def json(self):
        return {'type': 'deferred', 'busy': self.busy.json()}


@dataclasses.dataclass(frozen=True)
class Rejected:
    reason: str

    def __post_init__(self):
        require(self.reason in REJECT, 'invalid rejection')

    def json(self):
        return {'type': 'rejected', 'reason': self.reason}


@dataclasses.dataclass(frozen=True)
class Applied:
    previous: dict
    current: dict

    def json(self):
        return {'type': 'applied', 'previous': self.previous, 'current': self.current}


@dataclasses.dataclass(frozen=True)
class RolledBack:
    previous: dict
    failed: dict
    method: str

    def json(self):
        return {'type': 'rolled_back', 'previous': self.previous, 'failed': self.failed, 'method': self.method}


@dataclasses.dataclass(frozen=True)
class Failed:
    detail: str

    def json(self):
        return {'type': 'failed', 'detail': self.detail}


@dataclasses.dataclass(frozen=True)
class ChannelPointer:
    identity: ReleaseIdentity
    release: str
    manifest_sha256: str


@dataclasses.dataclass(frozen=True)
class Attempt:
    started_at: int
    finished_at: int
    trigger: str
    outcome: object

    def json(self):
        return {'started_at': self.started_at, 'finished_at': self.finished_at,
                'trigger': self.trigger, 'outcome': self.outcome.json()}


@dataclasses.dataclass
class Effects:
    fetch_pointer: object
    download: object
    version: object
    idle: object
    stop: object
    start: object
    install: object
    reinstall: object
    health: object
    restore: object
    now: object


def parse_idle(value):
    require(isinstance(value, dict), 'invalid idle')
    if value.get('idle') is True and set(value) == {'idle'}:
        return Idle()
    require(value.get('idle') is False and set(value) == {'idle', 'reason'}, 'invalid idle')
    return Busy(value['reason'])


def parse_pointer(value):
    require(set(value) == {'identity', 'release', 'manifest_sha256'}, 'invalid channel pointer')
    require(re.fullmatch(r'[A-Za-z0-9._-]{1,120}', value['release']) is not None, 'invalid release tag')
    require(re.fullmatch(r'[0-9a-f]{64}', value['manifest_sha256']) is not None, 'invalid manifest digest')
    return ChannelPointer(ReleaseIdentity.parse(value['identity']), value['release'], value['manifest_sha256'])


def plan(current, pointer, idle):
    if current == pointer:
        return Current()
    if isinstance(idle, Busy):
        return Deferred(idle)
    return Ready()


def verify_set(directory, key):
    try:
        return verify(directory, trusted_key=key)
    except subprocess.CalledProcessError:
        return Rejected('signature')
    except ValueError as error:
        text = str(error)
        if any(word in text for word in ('corrupt', 'unexpected', 'unsafe', 'missing', 'symlink')):
            return Rejected('tamper')
        return Rejected('identity')


def rollback_method(previous_manifest, target_manifest, component):
    key = 'manager_schema' if component == 'manager' else 'worker_schema'
    previous = previous_manifest['data_compatibility'][key]
    target = target_manifest['data_compatibility'][key]
    if previous == target:
        return 'reinstall_package'
    return 'restore_backup'


def health(url, timeout=20, pause=0.25):
    deadline = time.monotonic() + timeout
    while True:
        try:
            request = urllib.request.Request(url, headers={'User-Agent': 'botstral-update'})
            with urllib.request.urlopen(request, timeout=2) as response:
                body = json.loads(response.read().decode())
                if response.status == 200 and set(body) == {'status'} and body['status'] == 'ok':
                    return Probe.OK
        except (OSError, ValueError):
            pass
        if time.monotonic() >= deadline:
            return Probe.FAIL
        time.sleep(pause)


def policy(current, target):
    try:
        replacement_policy(current, target, switch_channel=False)
    except ValueError as error:
        text = str(error)
        if 'downgrade' in text:
            return Rejected('downgrade')
        if 'channel' in text:
            return Rejected('channel')
        return Rejected('identity')
    return None


def asset_url(channel_url, tag, name):
    prefix, separator, _rest = channel_url.rpartition('/')
    require(separator == '/', 'channel URL has no release directory')
    root, separator, _channel = prefix.rpartition('/')
    require(separator == '/', 'channel URL has no download root')
    return root + '/' + tag + '/' + name


def contained_file(directory, name):
    require(isinstance(name, str) and SAFE_NAME.fullmatch(name) is not None, 'unsafe artifact name')
    path = directory / name
    require(path.resolve().parent == directory.resolve(), 'artifact escaped download directory')
    return path


def verify_signature(directory, key):
    subprocess.run(
        ['openssl', 'dgst', '-sha256', '-verify', str(key),
         '-signature', str(contained_file(directory, 'manifest.sig')),
         str(contained_file(directory, 'manifest.json'))],
        check=True, capture_output=True,
    )


def download_release(channel_url, pointer, directory, fetch, trusted_key):
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    contained_file(directory, 'manifest.json').write_bytes(
        fetch(asset_url(channel_url, pointer.release, 'manifest.json')))
    contained_file(directory, 'manifest.sig').write_bytes(
        fetch(asset_url(channel_url, pointer.release, 'manifest.sig')))
    verify_signature(directory, trusted_key)
    parsed = json.loads(contained_file(directory, 'manifest.json').read_text())
    require(isinstance(parsed, dict) and isinstance(parsed.get('artifacts'), list), 'invalid manifest')
    for item in parsed['artifacts']:
        require(isinstance(item, dict), 'invalid artifact')
        name = item.get('filename')
        contained_file(directory, name).write_bytes(fetch(asset_url(channel_url, pointer.release, name)))


def write_atomic(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.tmp')
    data = json.dumps(value, sort_keys=True, indent=2) + '\n'
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, 'w') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    try:
        parent = path.parent.stat()
        os.chown(path, parent.st_uid, parent.st_gid)
    except OSError:
        pass


def replace_tree(source, destination):
    temporary = destination.with_name(destination.name + '.next')
    retired = destination.with_name(destination.name + '.old')
    if temporary.exists():
        shutil.rmtree(temporary)
    shutil.copytree(source, temporary)
    if retired.exists():
        shutil.rmtree(retired)
    if destination.exists():
        destination.rename(retired)
    temporary.rename(destination)
    if retired.exists():
        shutil.rmtree(retired)


def snapshot_state(state_dir, destination):
    destination.mkdir(parents=True)
    names = ('manager.sqlite3', 'manager.sqlite3-wal', 'manager.sqlite3-shm',
             'agent.sqlite3', 'agent.sqlite3-wal', 'agent.sqlite3-shm', 'config.json')
    for name in names:
        source = state_dir / name
        if source.is_file() and not source.is_symlink():
            shutil.copy2(source, destination / name)


def hold_lock(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    handle = open(path, 'a+')
    try:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        handle.close()
        return None
    return handle


def report_for(installed, started, finished, trigger, outcome):
    return {'installed': installed, 'last': Attempt(started, finished, trigger, outcome).json()}


def finish(path, installed, started, effects, trigger, outcome):
    write_atomic(path, report_for(installed, started, effects.now(), trigger, outcome))
    return 0 if not isinstance(outcome, Failed) else 1


def previous_deb(release_dir, component):
    for item in read_json(release_dir / 'manifest.json')['artifacts']:
        if item.get('component') == component:
            path = release_dir / item['filename']
            require(path.is_file() and not path.is_symlink(), 'previous package missing')
            return path
    raise ValueError('previous package missing')


def reinstall_previous(release_dir, component):
    package = 'botstral-' + component
    deb = previous_deb(release_dir, component)
    subprocess.run(['dpkg', '--remove', package], check=True)
    subprocess.run(['dpkg', '--install', str(deb)], check=True)


def perform_rollback(effects, options, backup, previous_manifest, target_manifest, previous_version):
    method = rollback_method(previous_manifest, target_manifest, options.component)
    if method == 'restore_backup':
        effects.restore(backup / 'state', backup / 'config')
    effects.reinstall(backup / 'release', options.component, previous_version)
    return method


def run(options, effects):
    lock = hold_lock(options.state_dir / 'update.lock')
    if lock is None:
        return 0
    try:
        return _run(options, effects)
    finally:
        lock.close()


def _run(options, effects):
    started = effects.now()
    request = options.state_dir / 'update.request'
    trigger = 'now' if options.now or request.exists() else 'timer'
    if request.exists():
        request.unlink()
    installed = effects.version()
    if not installed or 'identity' not in installed:
        return finish(options.report, installed or {}, started, effects, trigger,
                      Failed('installed identity unavailable'))
    try:
        current = ReleaseIdentity.parse(installed['identity'])
        pointer = parse_pointer(effects.fetch_pointer(options.channel_url))
    except (OSError, ValueError, KeyError, TypeError) as error:
        return finish(options.report, installed, started, effects, trigger, Failed(str(error)))
    decided = plan(current, pointer.identity, Idle())
    if isinstance(decided, Current):
        return finish(options.report, installed, started, effects, trigger, decided)
    incoming = options.state_dir / 'update-incoming'
    try:
        effects.download(options.channel_url, pointer, incoming)
        checked = verify_set(incoming, options.trusted_key)
        if isinstance(checked, Rejected):
            return finish(options.report, installed, started, effects, trigger, checked)
        if digest(incoming / 'manifest.json') != pointer.manifest_sha256 or checked.identity != pointer.identity:
            return finish(options.report, installed, started, effects, trigger, Rejected('identity'))
        rejected = policy(current, checked.identity)
        if rejected is not None:
            return finish(options.report, installed, started, effects, trigger, rejected)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        return finish(options.report, installed, started, effects, trigger, Failed(str(error)))
    try:
        idle = parse_idle(effects.idle())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        return finish(options.report, installed, started, effects, trigger, Failed(str(error)))
    decided = plan(current, pointer.identity, idle)
    if isinstance(decided, Deferred):
        return finish(options.report, installed, started, effects, trigger, decided)
    previous_set = options.state_dir / 'installed-release'
    backup = options.state_dir / 'update-backup'
    if backup.exists():
        shutil.rmtree(backup)
    backup.mkdir()
    if (previous_set / 'manifest.json').is_file():
        shutil.copytree(previous_set, backup / 'release')
    else:
        (backup / 'release').mkdir()
    (backup / 'config').mkdir()
    snapshot_state(options.state_dir, backup / 'state')
    write_json(backup / 'backup.json', {'stopped_at': effects.now(), 'manager': options.component == 'manager',
                                        'worker': options.component == 'agent'})
    try:
        effects.stop()
        receipt = backup / 'apply-receipt.json'
        effects.install(incoming, options.component, checked.identity.version,
                        backup / 'backup.json', backup / 'config', receipt)
        effects.start()
        if effects.health(options.listen) != Probe.OK:
            effects.stop()
            method = perform_rollback(effects, options, backup, read_json(backup / 'release' / 'manifest.json'),
                                      checked.value, current.version)
            effects.start()
            restored = effects.version() or installed
            return finish(options.report, restored, started, effects, trigger,
                          RolledBack(current.json(), checked.identity.json(), method))
        replace_tree(incoming, previous_set)
        applied = effects.version() or installed
        return finish(options.report, applied, started, effects, trigger,
                      Applied(current.json(), checked.identity.json()))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        try:
            if (backup / 'release' / 'manifest.json').is_file():
                effects.stop()
                perform_rollback(effects, options, backup, read_json(backup / 'release' / 'manifest.json'),
                                 checked.value, current.version)
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            pass
        try:
            effects.start()
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            pass
        return finish(options.report, effects.version() or installed, started, effects, trigger, Failed(str(error)))


def fetch_bytes(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'botstral-update'})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def host_idle(component, state_dir):
    path = state_dir / ('manager.sqlite3' if component == 'manager' else 'agent.sqlite3')
    if path.is_symlink() or not path.is_file():
        return {'idle': True}
    connection = sqlite3.connect('file:' + path.resolve().as_posix() + '?mode=ro', uri=True, timeout=2)
    try:
        if component == 'manager':
            count, = connection.execute(
                "SELECT count(*) FROM tasks WHERE json_extract(view,'$.state') IN ('running','cancelling')"
            ).fetchone()
            if count:
                return {'idle': False, 'reason': 'active_task'}
            return {'idle': True}
        for (record,) in connection.execute('SELECT record FROM executions'):
            process = json.loads(record).get('process')
            if not isinstance(process, dict):
                continue
            lifecycle = process.get('lifecycle')
            kind = lifecycle.get('type') if isinstance(lifecycle, dict) else None
            if kind != 'stopped':
                return {'idle': False, 'reason': 'active_process'}
        return {'idle': True}
    finally:
        connection.close()


def production_effects(component, state_dir, trusted_key):
    binary = Path('/usr/bin/botstral-' + component)

    def version():
        if not binary.is_file():
            return None
        return json.loads(subprocess.check_output([str(binary), '--version-json']))

    def idle():
        return host_idle(component, state_dir)

    def systemctl(action):
        subprocess.run(host_service(component, state_dir, action), check=True)

    def install(release_dir, selected, version, backup_receipt, config_backup, receipt):
        script = Path(__file__).resolve().parent
        upgrade = TOOLS / 'release_upgrade.py' if TOOLS.exists() else script / 'release_upgrade.py'
        subprocess.run([sys.executable, str(upgrade), str(release_dir), '--version', version,
                        '--component', selected, '--trusted-key', str(trusted_key),
                        '--backup-receipt', str(backup_receipt), '--config-backup', str(config_backup),
                        '--receipt', str(receipt), '--apply'], check=True)

    def reinstall(release_dir, selected, _version):
        reinstall_previous(release_dir, selected)

    def probe(listen):
        if component == 'agent':
            command = host_service(component, state_dir, 'is-active')
            command.insert(-1, '--quiet')
            return Probe.OK if subprocess.run(command).returncode == 0 else Probe.FAIL
        return health('http://' + listen + '/health')

    def restore(state_backup, config_backup):
        config = Path('/etc/botstral-' + component)
        if any(config_backup.iterdir()):
            if config.exists():
                shutil.rmtree(config)
            shutil.copytree(config_backup, config)
        for item in state_backup.iterdir():
            shutil.copy2(item, state_dir / item.name)

    return Effects(lambda url: json.loads(fetch_bytes(url)),
                   lambda url, pointer, directory: download_release(
                       url, pointer, directory, fetch_bytes, trusted_key),
                   version, idle, lambda: systemctl('stop'), lambda: systemctl('start'),
                   install, reinstall, probe, restore, lambda: int(time.time()))


def host_service(component, state_dir, action):
    unit = 'botstral-' + component + '.service'
    if component == 'manager':
        return ['systemctl', action, unit]
    user = pwd.getpwuid(state_dir.stat().st_uid).pw_name
    return ['systemctl', '--user', '--machine=' + user + '@', action, unit]


def load_key(component):
    config = read_config(component)
    return Path(config.get('trusted_key', '/etc/botstral-' + component + '/manifest-public.pem'))


def read_config(component):
    path = Path('/etc/botstral-' + component) / 'update.json'
    if path.is_file() and not path.is_symlink():
        return read_json(path)
    return {}


def listen_address(component):
    service = Path('/etc/botstral-' + component) / 'service.json'
    if component == 'manager' and service.is_file() and not service.is_symlink():
        value = read_json(service).get('listen')
        if isinstance(value, str):
            return value
    return '127.0.0.1:43120'


def main(argv=None):
    parser = argparse.ArgumentParser(description='Check the channel and update this host when it is idle.')
    parser.add_argument('--now', action='store_true')
    parser.add_argument('--component', choices=['manager', 'agent'])
    parser.add_argument('--state-dir', type=Path)
    parser.add_argument('--channel-url')
    parser.add_argument('--trusted-key', type=Path)
    args = parser.parse_args(argv)
    component = args.component
    if component is None:
        parser.error('--component is required')
    configured = read_config(component)
    if args.state_dir:
        state_dir = args.state_dir
    elif component == 'manager':
        state_dir = Path('/var/lib/botstral-manager')
    elif os.getuid() == 0:
        state_dir = Path('/home/botstral-agent/.local/share/botstral-agent')
    else:
        state_dir = Path.home() / '.local/share/botstral-agent'
    options = argparse.Namespace(
        component=component,
        state_dir=state_dir,
        channel_url=args.channel_url or configured.get('channel_url', CHANNEL),
        trusted_key=args.trusted_key or load_key(component),
        report=state_dir / 'update-report.json',
        listen=listen_address(component),
        now=args.now,
    )
    try:
        return run(options, production_effects(component, state_dir, options.trusted_key))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print('update failed: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
