#!/usr/bin/env python3
DESCRIPTION = 'Run native old-to-new package acceptance on a disposable host, without publishing.'
import argparse
from pathlib import Path
import subprocess
import sys

from release_model import Artifact, ReleaseIdentity, authority, digest, read_json, require, write_json

ROOT = Path(__file__).resolve().parents[1]


def built(directory, target):
    directory = Path(directory).resolve()
    descriptors = [read_json(p) for p in directory.glob('*.artifact.json')]
    require(descriptors, 'built artifact descriptors required')
    identity = ReleaseIdentity.parse(descriptors[0]['identity'])
    artifacts = [Artifact.parse(d, identity, authority()['artifacts']) for d in descriptors]
    require(sorted((a.component, a.target) for a in artifacts) == sorted((e['component'], e['target']) for e in authority()['artifacts'] if e['target'] == target), 'complete native target set required')
    for artifact in artifacts:
        path = directory / artifact.filename
        require(not path.is_symlink() and path.is_file() and path.stat().st_size == artifact.size and digest(path) == artifact.sha256, 'native artifact digest mismatch')
    return identity, artifacts


def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def refused(args, expected, **kwargs):
    result = subprocess.run([str(a) for a in args], capture_output=True, text=True, **kwargs)
    require(result.returncode != 0 and expected in result.stdout + result.stderr, 'negative control did not refuse with ' + expected + ': ' + result.stdout + result.stderr)


def main():
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    parser.add_argument('--old', type=Path, required=True)
    parser.add_argument('--new', type=Path, required=True)
    parser.add_argument('--target', choices=['ubuntu24.04-amd64', 'macos-arm64'], required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--production', action='store_true')
    parser.add_argument('--package-only', action='store_true', help='Explicit local Linux subset without systemd diagnostics or worker supervisor acceptance.')
    args = parser.parse_args()
    require(not args.output.exists(), 'acceptance output must be new')
    require(not (args.production and args.package_only), 'package-only results cannot establish production acceptance')
    require(not args.package_only or args.target == 'ubuntu24.04-amd64', 'package-only mode is for Linux containers')
    old, old_artifacts = built(args.old, args.target)
    new, artifacts = built(args.new, args.target)
    require(old != new and old.version != new.version, 'acceptance requires two distinct executable release identities')
    from release_upgrade import replacement_policy
    replacement_policy(old, new, switch_channel=True)
    if args.target == 'ubuntu24.04-amd64':
        from release_acceptance_linux import accept
    else:
        from release_acceptance_macos import accept
    evidence = accept(args, old, new, old_artifacts, artifacts)
    write_json(args.output, {'identity': new.json(), 'previous': old.json(), 'target': args.target,
                            'artifacts': {a.filename: a.sha256 for a in artifacts},
                            'passed': True, 'production': args.production, 'coverage': 'package-only' if args.package_only else 'native', 'evidence': evidence})


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print('native acceptance FAILED: ' + str(error), file=sys.stderr)
        sys.exit(1)
