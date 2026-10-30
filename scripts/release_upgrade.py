#!/usr/bin/env python3
DESCRIPTION = 'Verify an exact release and record an offline upgrade before native replacement.'
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

from release_model import ReleaseIdentity, digest, embedded, read_json, require, verify, write_json
from release_artifacts import inspect_artifacts, inspect_app, mount_dmg, app_team


def version_key(identity):
    base, _, nightly = identity.version.partition('-')
    numbers = tuple(int(v) for v in base.split('.'))
    return numbers + ((0,) + tuple(int(v) for v in nightly.split('.')[1:]) if nightly else (1, 0, 0))


def replacement_policy(previous, target, switch_channel=False):
    require(version_key(target) >= version_key(previous), 'unsafe downgrade; restore a coordinated backup with old binaries instead')
    require(previous.channel == target.channel or switch_channel, 'channel switch requires --switch-channel')
    require(previous.version != target.version or previous == target, 'same product version has a different identity')


def stopped(executables):
    if sys.platform == 'linux':
        require(Path('/proc/self/exe').exists(), 'procfs required')
        for process in Path('/proc').iterdir():
            if not process.name.isdecimal():
                continue
            try:
                path = os.readlink(str(process / 'exe')).removesuffix(' (deleted)')
                arguments = (process / 'cmdline').read_bytes().split(b'\0')
            except FileNotFoundError:
                continue
            require(path not in executables and not any(os.fsencode(p) in arguments for p in executables), 'active replacement refused; stop and back up first')
    else:
        rows = subprocess.check_output(['ps', '-axo', 'comm='], text=True).splitlines()
        require(not any(row.strip() in executables for row in rows), 'active desktop replacement refused; quit the app first')


def prepare(args):
    release = verify(args.directory, args.trusted_key, args.unsigned, production=not args.unsigned)
    require(release.identity.version == args.version, 'exact requested version mismatch')
    target = 'macos-arm64' if args.component == 'desktop' else 'ubuntu24.04-amd64'
    require((sys.platform == 'darwin' and platform.machine() == 'arm64') if args.component == 'desktop' else (sys.platform == 'linux' and platform.machine() == 'x86_64'), 'wrong installation platform or architecture')
    artifact = next(a for a in release.artifacts if a.component == args.component and a.target == target)
    inspect_artifacts(args.directory, release, production=not args.unsigned)
    installed = args.app_path / 'Contents/MacOS/botstral-desktop' if args.component == 'desktop' else Path('/usr/bin/botstral-' + args.component)
    paths = {str(installed)}
    if args.component == 'agent':
        paths.add(str(installed) + '-supervisor-check')
    stopped(paths)
    previous = None
    if args.repair_receipt:
        receipt = read_json(args.repair_receipt)
        require(receipt.get('phase') == 'prepared', 'repair requires an incomplete prepared upgrade')
        require(receipt.get('installation_path') == str(installed.absolute()), 'repair receipt belongs to a different installation')
        require(receipt['target'] == release.identity.json() and receipt['component'] == args.component, 'repair receipt does not match exact target')
        require(receipt['artifact_sha256'] == artifact.sha256, 'repair artifact differs from prepared upgrade')
        previous = ReleaseIdentity.parse(receipt['previous']) if receipt['previous'] else None
        if previous:
            replacement_policy(previous, release.identity, args.switch_channel)
        if installed.exists():
            require(not installed.is_symlink(), 'unmanaged symlink refused')
            try:
                current = ReleaseIdentity.parse(embedded(installed)['identity'])
            except (ValueError, KeyError, TypeError):
                current = None
            if current:
                replacement_policy(current, release.identity, args.switch_channel)
    elif installed.exists():
        require(not installed.is_symlink(), 'unmanaged symlink refused')
        previous = ReleaseIdentity.parse(embedded(installed)['identity'])
        if args.component != 'desktop':
            owner = subprocess.check_output(['dpkg-query', '-S', str(installed)], text=True)
            require(owner.startswith('botstral-' + args.component + ': '), 'unmanaged installation adoption refused')
        replacement_policy(previous, release.identity, args.switch_channel)
    else:
        require(args.fresh_install, 'missing installed identity; use --fresh-install only for a new installation')
        if args.component == 'desktop':
            require(not args.app_path.exists(), 'damaged existing bundle requires --repair-receipt')
    backup = None
    if previous:
        require(args.backup_receipt and args.config_backup, 'coordinated offline backup receipt and configuration backup required')
        backup = read_json(args.backup_receipt)
        require(all(key in backup for key in ('stopped_at', 'manager', 'worker')), 'incomplete coordinated backup receipt')
        require(args.config_backup.is_dir() and not args.config_backup.is_symlink(), 'configuration backup directory required')
    receipt = {'component': args.component, 'phase': 'prepared', 'installation_path': str(installed.absolute()),
               'previous': previous.json() if previous else None,
               'target': release.identity.json(), 'artifact_sha256': artifact.sha256,
               'backup_receipt': str(args.backup_receipt.resolve()) if args.backup_receipt else None,
               'backup_receipt_sha256': digest(args.backup_receipt) if args.backup_receipt else None,
               'configuration_backup': str(args.config_backup.resolve()) if args.config_backup else None,
               'production': not args.unsigned, 'services_started': False}
    require(not args.receipt.exists(), 'upgrade receipt must be new')
    with open(args.receipt, 'x', opener=lambda path, flags: os.open(path, flags, 0o600)) as stream:
        json.dump(receipt, stream, sort_keys=True, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    return release, artifact, paths


def replace_app(dmg, destination, identity, production):
    require(destination.name == 'Botstral.app' and destination.is_absolute(), 'absolute Botstral.app destination required')
    require(not any(p.is_symlink() for p in [destination, *destination.parents]), 'symlink app destination refused')
    with tempfile.TemporaryDirectory(prefix='botstral-install-') as directory:
        mount = Path(directory) / 'volume'
        mount_dmg(dmg, mount)
        try:
            source = mount / 'Botstral.app'
            inspect_app(source, identity, production)
            if production and (destination / 'Contents/MacOS/botstral-desktop').exists():
                require(app_team(destination) == app_team(source), 'desktop signing team changed; credential continuity is not established')
            if destination.exists():
                retired = destination.with_name('Botstral.app.retired-' + identity.build_id)
                require(not retired.exists(), 'retired bundle already exists; inspect previous interrupted replacement')
                destination.rename(retired)
            subprocess.run(['ditto', str(source), str(destination)], check=True)
            inspect_app(destination, identity, production)
        finally:
            subprocess.run(['hdiutil', 'detach', str(mount)], check=True, capture_output=True)


def main():
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--version', required=True)
    parser.add_argument('--component', choices=['manager', 'agent', 'desktop'], required=True)
    parser.add_argument('--trusted-key', type=Path)
    parser.add_argument('--unsigned', action='store_true', help='Local acceptance only. Never production.')
    parser.add_argument('--fresh-install', action='store_true')
    parser.add_argument('--switch-channel', action='store_true')
    parser.add_argument('--backup-receipt', type=Path)
    parser.add_argument('--config-backup', type=Path)
    parser.add_argument('--repair-receipt', type=Path)
    parser.add_argument('--receipt', type=Path, required=True)
    parser.add_argument('--app-path', type=Path, default=Path('/Applications/Botstral.app'))
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    release, artifact, paths = prepare(args)
    if args.apply:
        stopped(paths)
        path = args.directory / artifact.filename
        require(digest(path) == artifact.sha256, 'artifact changed after preflight')
        if args.component == 'desktop':
            replace_app(path, args.app_path, release.identity, not args.unsigned)
        else:
            subprocess.run(['dpkg', '--install', str(path.resolve())], check=True)
        installed = args.app_path / 'Contents/MacOS/botstral-desktop' if args.component == 'desktop' else Path('/usr/bin/botstral-' + args.component)
        actual = json.loads(subprocess.check_output([str(installed), '--version-json']))
        require(actual['identity'] == release.identity.json(), 'installed identity confirmation failed; leave services stopped')
        receipt = read_json(args.receipt)
        receipt['phase'] = 'applied'
        with args.receipt.open('r+') as stream:
            json.dump(receipt, stream, sort_keys=True, indent=2)
            stream.write('\n')
            stream.truncate()
            stream.flush()
            os.fsync(stream.fileno())
    print(json.dumps({'receipt': str(args.receipt), 'applied': args.apply, 'services_started': False}))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print('upgrade refused; keep services stopped: ' + str(error), file=sys.stderr)
        sys.exit(1)
