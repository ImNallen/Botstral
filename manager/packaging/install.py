#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys


def install(component, binary, root, source=None, identity=None):
    from recovery import absolute, require
    root = Path('/') if str(root) == '/' else absolute(root)
    other = 'agent' if component == 'manager' else 'manager'
    other_library = absolute(root / ('usr/lib/botstral-' + other))
    require(not other_library.exists(), 'manager and worker require separate installation roots')
    binary = absolute(binary)
    source = source or Path(__file__).resolve().parent
    require(binary.is_file() and not binary.is_symlink(), 'regular built binary required')
    source_id = hashlib.sha256(binary.read_bytes()).hexdigest()
    supervisor = binary.with_name('botstral-agent-supervisor-check')
    if component == 'agent':
        require(supervisor.is_file() and not supervisor.is_symlink(), 'built production supervisor check binary required')
    library = absolute(root / ('usr/lib/botstral-' + component))
    require(not library.exists(), 'component already installed; stage a new package')
    library.mkdir(parents=True, mode=0o755)
    destination = absolute(root / ('usr/bin/botstral-' + component))
    destination.parent.mkdir(parents=True, exist_ok=True)
    require(not destination.exists(), 'binary already exists')
    shutil.copyfile(binary, destination)
    destination.chmod(0o755)
    if component == 'agent':
        check = destination.with_name('botstral-agent-supervisor-check')
        require(not check.exists() and not check.is_symlink(), 'supervisor check already installed')
        shutil.copyfile(supervisor, check)
        check.chmod(0o755)
    shared = source if (source / 'recovery.py').exists() else source.parents[1] / 'manager/packaging'
    for name in ('recovery.py', 'diagnose.py'):
        shutil.copyfile(shared / name, library / name)
        (library / name).chmod(0o644)
    shutil.copyfile(source / 'backup.py', library / 'backup.py')
    (library / 'backup.py').chmod(0o755)
    maintenance = destination.with_name(destination.name + '-backup')
    require(not maintenance.exists(), 'maintenance command already exists')
    maintenance.symlink_to('/usr/lib/botstral-' + component + '/backup.py')
    diagnostic = destination.with_name(destination.name + '-diagnose')
    require(not diagnostic.exists() and not diagnostic.is_symlink(), 'diagnostic command already exists')
    diagnostic.write_text('#!/bin/sh\nexec /usr/bin/python3 /usr/lib/botstral-' + component + '/diagnose.py ' + component + ' "$@"\n')
    diagnostic.chmod(0o755)
    unit = 'botstral-' + component + '.service'
    unit_dir = absolute(root / ('usr/lib/systemd/system' if component == 'manager' else 'usr/lib/systemd/user'))
    unit_dir.mkdir(parents=True, exist_ok=True)
    require(not (unit_dir / unit).exists() and not (unit_dir / unit).is_symlink(), 'service unit already exists')
    shutil.copyfile(source / unit, unit_dir / unit)
    (unit_dir / unit).chmod(0o644)
    if component == 'manager':
        shutil.copyfile(source / 'launch.py', library / 'launch.py')
        (library / 'launch.py').chmod(0o755)
        configuration = absolute(root / 'etc/botstral-manager')
        configuration.mkdir(parents=True, mode=0o700)
        shutil.copyfile(source / 'service.example.json', configuration / 'service.example.json')
        (configuration / 'service.example.json').chmod(0o600)
    else:
        configuration = absolute(root / 'etc/botstral-agent')
        configuration.mkdir(parents=True, mode=0o700)
        shutil.copyfile(source / 'recovery-plan.example.json', configuration / 'recovery-plan.example.json')
        (configuration / 'recovery-plan.example.json').chmod(0o600)
    release = identity or json.loads((Path(__file__).resolve().parents[2] / 'release.json').read_text())
    manifest = {'component': component, 'binary_sha256': source_id, 'version': release['version'], 'services_enabled': False, 'runtime': {'python': '3.9 or newer', 'sqlite': '3.37 or newer', 'systemd': '255 for worker', 'rust_build': '1.93.0', 'dependencies': 'Cargo.lock'}}
    (library / 'install.json').write_text(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
    (library / 'install.json').chmod(0o644)
    return manifest


def main(component=None, source=None):
    parser = argparse.ArgumentParser(description='Stage a package. Account creation and activation are separate explicit steps.')
    if component is None:
        parser.add_argument('component', choices=['manager', 'agent'])
    parser.add_argument('--binary', required=True)
    parser.add_argument('--root', required=True)
    parser.add_argument('--identity', type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(install(component or args.component, args.binary, args.root, source, json.loads(args.identity.read_text()) if args.identity else None)))
    except (OSError, ValueError) as error:
        print('installation refused: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main('manager'))
