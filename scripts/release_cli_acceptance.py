#!/usr/bin/env python3
"""Exercise the exact-version installer against a complete release set on a temporary Mac path."""
import argparse
import json
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

from release_acceptance import run, refused
from release_model import ROOT, require, verify, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--trusted-key', type=Path)
    parser.add_argument('--unsigned', action='store_true')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(sys.platform == 'darwin' and platform.machine() == 'arm64', 'Apple silicon CLI acceptance required')
    require(not args.output.exists(), 'CLI acceptance output must be new')
    release = verify(args.directory, args.trusted_key, args.unsigned, not args.unsigned)
    with tempfile.TemporaryDirectory(prefix='botstral-cli-') as temporary:
        root = Path(temporary).resolve()
        app = root / 'Botstral.app'
        command = [sys.executable, ROOT / 'scripts/release_upgrade.py', args.directory.resolve(),
                   '--component', 'desktop', '--app-path', app]
        command += ['--unsigned'] if args.unsigned else ['--trusted-key', args.trusted_key.resolve()]
        refused([*command, '--version', '0.0.0', '--fresh-install', '--receipt', root / 'wrong-version.json'], 'exact requested version mismatch')
        exact = [*command, '--version', release.identity.version]
        run([*exact, '--fresh-install', '--receipt', root / 'installed.json', '--apply'])
        require((root / 'installed.json').stat().st_mode & 0o777 == 0o600, 'upgrade receipt must be private')
        executable = app / 'Contents/MacOS/botstral-desktop'
        actual = json.loads(subprocess.check_output([str(executable), '--version-json']))
        require(actual['identity'] == release.identity.json(), 'installer did not confirm actual packaged identity')
        process = subprocess.Popen([str(executable), '--release-acceptance-hold'], stdout=subprocess.PIPE, text=True)
        try:
            require(process.stdout.readline().strip() == 'release acceptance process ready', 'native CLI readiness missing')
            refused([*exact, '--receipt', root / 'active.json', '--apply'], 'active desktop replacement refused')
        finally:
            process.terminate()
            process.wait(timeout=15)
            process.stdout.close()
        refused([*exact, '--receipt', root / 'missing-backup.json', '--apply'], 'coordinated offline backup receipt')
    write_json(args.output, {'identity': release.identity.json(), 'passed': True, 'production': not args.unsigned,
                            'exact_install_and_binary_confirmation': True, 'wrong_version_refused': True,
                            'active_replacement_refused': True, 'missing_backup_refused': True})


if __name__ == '__main__':
    main()
