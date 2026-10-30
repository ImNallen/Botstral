#!/usr/bin/env python3
DESCRIPTION = 'Build and verify immutable, exact-version Botstral release sets.'
import argparse
import datetime
import json
from pathlib import Path
import platform
import re
import subprocess
import sys

from release_model import (ROOT, ReleaseIdentity, ReleaseManifest, authority,
                           digest, read_json, require, verify, write_json)


def versions(check=True, identity=None, mirrors_only=False):
    version = identity.version if identity else authority()['version']
    changes = {}
    for name in ('manager', 'agent', 'link', 'link/client', 'desktop/src-tauri'):
        path = ROOT / name / 'Cargo.toml'
        changes[path] = re.sub(r'(?m)^version = "[^"]+"', 'version = "' + version + '"', path.read_text(), count=1)
    for name in ('desktop/package.json', 'link/package.json', 'desktop/src-tauri/tauri.conf.json', 'package-lock.json'):
        path = ROOT / name
        value = read_json(path)
        if name == 'package-lock.json':
            value['packages']['desktop']['version'] = version
            value['packages']['desktop']['dependencies']['@botstral/link'] = version
            value['packages']['link']['version'] = version
        else:
            value['version'] = version
            if name == 'desktop/package.json':
                value['dependencies']['@botstral/link'] = version
        changes[path] = json.dumps(value, indent=2) + '\n'
    for name in ('Cargo.lock', 'desktop/src-tauri/Cargo.lock'):
        path = ROOT / name
        changes[path] = re.sub(r'(name = "botstral-[^"]+"\nversion = ")[^"]+', lambda m: m[1] + version, path.read_text())
    if mirrors_only:
        return changes
    for path, wanted in changes.items():
        if check:
            actual = path.read_text()
            if path.suffix == '.json':
                require(json.loads(actual) == json.loads(wanted), 'version drift: ' + str(path.relative_to(ROOT)))
            else:
                require(actual == wanted, 'version drift: ' + str(path.relative_to(ROOT)))
        else:
            path.write_text(wanted)
    config = authority()
    for path, pattern, expected in [('link/src/lib.rs', r'pub const VERSION: u[0-9]+ = (\d+)', config['protocols']['worker']),
                                    ('link/src/owner.rs', r'pub const OWNER_VERSION: u[0-9]+ = (\d+)', config['protocols']['owner'])]:
        match = re.search(pattern, (ROOT / path).read_text())
        require(match is not None and int(match[1]) == expected, 'protocol mirror drift: ' + path)
    schemas = [int(p.stem) for p in (ROOT / 'manager/migrations').glob('*.sql')]
    require(max(schemas) == config['data_compatibility']['manager_schema'], 'manager data compatibility drift')
    journal = (ROOT / 'agent/src/journal.rs').read_text()
    require(max(int(v) for v in re.findall(r'PRAGMA user_version=(\d+)', journal)) == config['data_compatibility']['worker_schema'], 'worker data compatibility drift')
    require('channel = "' + config['runtime_requirements']['rust'] + '"' in (ROOT / 'rust-toolchain.toml').read_text(), 'Rust runtime evidence drift')
    require('"' + config['runtime_requirements']['vibe'].split(';')[0] + '"' in (ROOT / 'link/src/vibe.rs').read_text(), 'Vibe runtime evidence drift')
    for path, expected in config['runtime_requirements']['source_pins'].items():
        require(digest(ROOT / path) == expected, 'runtime source pin drift: ' + path)


def validate_release_ref(identity):
    if identity.channel == 'stable':
        tags = subprocess.check_output(['git', 'tag', '--points-at', identity.revision], cwd=ROOT, text=True).splitlines()
        require('v' + identity.version in tags, 'stable production requires its matching source tag')


def create_identity(channel, build_id, sequence, date, local):
    versions()
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if not local:
        require(not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=ROOT), 'clean source required')
        if channel == 'stable':
            tag = subprocess.check_output(['git', 'describe', '--exact-match', '--tags', 'HEAD'], cwd=ROOT, text=True).strip()
            require(tag == 'v' + authority()['version'], 'stable requires matching immutable version tag')
    version = authority()['version']
    if channel == 'nightly':
        require(sequence >= 1, 'nightly sequence must be positive')
        version += '-nightly.' + date + '.' + str(sequence)
    identity = ReleaseIdentity.parse({'version': version, 'channel': channel, 'revision': revision, 'build_id': build_id})
    identity.native_versions()
    return identity


def aggregate(inputs, output, receipts):
    import shutil
    config = authority()
    descriptors = [read_json(p) for directory in inputs for p in Path(directory).glob('*.artifact.json')]
    require(descriptors, 'no built artifacts supplied')
    identity = ReleaseIdentity.parse(descriptors[0]['identity'])
    value = {k: config[k] for k in ('format', 'protocols', 'data_compatibility', 'runtime_requirements')}
    value.update(identity=identity.json(), link_schema_sha256=digest(ROOT / 'link/schema/protocol.json'),
                 artifacts=descriptors, acceptance=[read_json(p) for p in receipts])
    release = ReleaseManifest.parse(value)
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    for artifact in release.artifacts:
        matches = [Path(directory) / artifact.filename for directory in inputs if (Path(directory) / artifact.filename).exists()]
        require(len(matches) == 1, 'missing or duplicate built artifact')
        source = matches[0]
        require(not source.is_symlink() and digest(source) == artifact.sha256 and source.stat().st_size == artifact.size, 'built artifact digest mismatch')
        shutil.copyfile(source, output / artifact.filename)
    write_json(output / 'manifest.json', value)
    verify(output, unsigned=True)


def main():
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    commands = parser.add_subparsers(dest='command', required=True)
    p = commands.add_parser('versions')
    modes = p.add_mutually_exclusive_group()
    modes.add_argument('--write', action='store_true')
    modes.add_argument('--check', action='store_true')
    p.add_argument('--identity', type=Path)
    p = commands.add_parser('identity')
    p.add_argument('--channel', choices=['stable', 'nightly'], required=True)
    p.add_argument('--build-id', required=True)
    p.add_argument('--sequence', type=int, default=1)
    p.add_argument('--date', default=datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d'))
    p.add_argument('--local', action='store_true', help='Allow uncommitted source for local verification only.')
    p.add_argument('--output', type=Path, required=True)
    p = commands.add_parser('verify')
    p.add_argument('directory', type=Path)
    p.add_argument('--trusted-key', type=Path)
    p.add_argument('--unsigned', action='store_true')
    p.add_argument('--production', action='store_true')
    p = commands.add_parser('aggregate')
    p.add_argument('--input', type=Path, action='append', required=True)
    p.add_argument('--receipt', type=Path, action='append', default=[])
    p.add_argument('--output', type=Path, required=True)
    p = commands.add_parser('sign')
    p.add_argument('directory', type=Path)
    p.add_argument('--private-key', type=Path, required=True)
    p.add_argument('--trusted-key', type=Path, required=True)
    p = commands.add_parser('build')
    p.add_argument('--identity', type=Path, required=True)
    p.add_argument('--target', choices=sorted({e['target'] for e in authority()['artifacts']}), required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--unsigned', action='store_true')
    args = parser.parse_args()
    if args.command == 'versions':
        versions(not args.write, ReleaseIdentity.parse(read_json(args.identity)) if args.identity else None)
    elif args.command == 'identity':
        require(not args.output.exists(), 'identity output must be new')
        write_json(args.output, create_identity(args.channel, args.build_id, args.sequence, args.date, args.local).json())
    elif args.command == 'verify':
        release = verify(args.directory, args.trusted_key, args.unsigned, args.production)
        from release_artifacts import inspect_artifacts
        inspect_artifacts(args.directory, release, args.production)
        print(json.dumps(release.identity.json()))
    elif args.command == 'aggregate':
        aggregate(args.input, args.output, args.receipt)
    elif args.command == 'sign':
        release = verify(args.directory, unsigned=True)
        ReleaseManifest.parse(release.value, production=True)
        require(release.identity.revision == subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), 'signing checkout differs from release source')
        validate_release_ref(release.identity)
        require(sys.platform == 'darwin' and platform.machine() == 'arm64', 'production signing requires native Apple silicon inspection')
        from release_artifacts import inspect_artifacts
        inspect_artifacts(args.directory, release, production=True)
        subprocess.run(['openssl', 'dgst', '-sha256', '-sign', str(args.private_key), '-out', str(args.directory / 'manifest.sig'), str(args.directory / 'manifest.json')], check=True)
        verify(args.directory, args.trusted_key, production=True)
    elif args.command == 'build':
        from release_package import build
        build(args.identity, args.target, args.output, args.unsigned)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print('release refused: ' + str(error), file=sys.stderr)
        sys.exit(1)
