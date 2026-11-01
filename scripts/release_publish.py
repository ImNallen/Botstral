#!/usr/bin/env python3
import datetime
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from release_model import digest, require, verify, write_json
from release_artifacts import inspect_artifacts


def gh(*args):
    subprocess.run(['gh', *map(str, args)], check=True)


def main():
    directory = Path(sys.argv[1])
    require(os.environ.get('GITHUB_ACTIONS') == 'true', 'publication is restricted to the gated CI job')
    require(os.environ.get('GITHUB_EVENT_NAME') in ('push', 'schedule', 'workflow_dispatch'), 'trusted publication event required')
    with tempfile.TemporaryDirectory() as temporary:
        key = Path(temporary) / 'trusted.pem'
        key.write_text(os.environ['PUBLIC_KEY'])
        release = verify(directory, key, production=True)
        inspect_artifacts(directory, release, production=True)
        identity = release.identity
        require(identity.revision == subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(), 'publication checkout differs from signed source revision')
        tag = ('v' if identity.channel == 'stable' else 'nightly-') + identity.version
        existing = subprocess.run(['gh', 'release', 'view', tag], capture_output=True)
        require(existing.returncode != 0, 'immutable release already exists; do not overwrite it')
        files = [directory / 'manifest.json', directory / 'manifest.sig', *[directory / a.filename for a in release.artifacts]]
        options = ['--verify-tag'] if identity.channel == 'stable' else ['--prerelease', '--target', identity.revision]
        gh('release', 'create', tag, '--draft', '--latest=false', '--title', 'Botstral ' + identity.version, '--notes', 'Exact release set. See docs/releases.md at the recorded source revision.', *options, *files)
        gh('release', 'edit', tag, '--draft=false', '--latest=false')
        channel = 'channel-' + identity.channel
        pointer = Path(temporary) / 'channel.json'
        write_json(pointer, {'identity': identity.json(), 'release': tag, 'manifest_sha256': digest(directory / 'manifest.json')})
        if subprocess.run(['gh', 'release', 'view', channel], capture_output=True).returncode:
            gh('release', 'create', channel, '--prerelease', '--latest=false', '--target', identity.revision, '--title', identity.channel + ' channel', '--notes', 'Discovery pointer only. Install a verified immutable exact release set.')
        gh('release', 'upload', channel, pointer, '--clobber')
        notes = 'Botstral ' + identity.version
        published = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
        tarball = directory.parent / 'Botstral.app.tar.gz'
        signature_path = directory.parent / 'Botstral.app.tar.gz.sig'
        signature = None
        url = None
        if tarball.is_file() and signature_path.is_file() and not tarball.is_symlink() and not signature_path.is_symlink():
            gh('release', 'upload', channel, tarball, signature_path, '--clobber')
            signature = signature_path.read_text().strip()
            url = 'https://github.com/ImNallen/Botstral/releases/download/' + channel + '/' + tarball.name
        latest = Path(temporary) / 'latest.json'
        platforms = {}
        if signature is not None and url is not None:
            platforms['darwin-aarch64'] = {'signature': signature, 'url': url}
        write_json(latest, {'version': identity.version, 'notes': notes, 'pub_date': published, 'platforms': platforms})
        gh('release', 'upload', channel, latest, '--clobber')
        if identity.channel == 'stable':
            gh('release', 'edit', tag, '--latest')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        sys.exit('publication refused: ' + str(error))
