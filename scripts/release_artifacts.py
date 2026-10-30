"""Inspect package contents without executing privileged installation scripts."""
import io
import plistlib
from pathlib import Path
import struct
import subprocess
import sys
import tarfile
import tempfile

from release_model import embedded, require


def ar_members(path):
    result = {}
    with Path(path).open('rb') as stream:
        require(stream.read(8) == b'!<arch>\n', 'invalid Debian archive')
        while True:
            header = stream.read(60)
            if not header:
                break
            require(len(header) == 60 and header[58:] == b'`\n', 'invalid archive header')
            size = int(header[48:58])
            name = header[:16].decode('ascii').strip().rstrip('/')
            require(name not in result and name in ('debian-binary', 'control.tar.xz', 'data.tar.xz'), 'unexpected Debian archive member')
            result[name] = stream.read(size)
            require(len(result[name]) == size, 'truncated archive')
            if size % 2:
                stream.read(1)
    require(set(result) == {'debian-binary', 'control.tar.xz', 'data.tar.xz'} and result['debian-binary'] == b'2.0\n', 'unsupported Debian archive layout')
    return result


def inspect_deb(path, artifact, release):
    members = ar_members(path)
    with tarfile.open(fileobj=io.BytesIO(members['control.tar.xz']), mode='r:xz') as archive:
        control = archive.extractfile('./control').read().decode()
    fields = dict(line.split(': ', 1) for line in control.splitlines() if ': ' in line)
    require(fields.get('Package') == 'botstral-' + artifact.component, 'wrong native component')
    require(fields.get('Architecture') == 'amd64', 'wrong native architecture')
    require(fields.get('Version') == artifact.native_versions['debian'], 'wrong native package version')
    expected = {'identity': release.identity.json(), 'protocols': release.value['protocols'],
                'link_schema_sha256': release.value['link_schema_sha256']}
    binaries = ['./usr/bin/botstral-' + artifact.component]
    if artifact.component == 'agent':
        binaries.append('./usr/bin/botstral-agent-supervisor-check')
    with tarfile.open(fileobj=io.BytesIO(members['data.tar.xz']), mode='r:xz') as archive:
        names = archive.getnames()
        require(len(names) == len(set(names)), 'duplicate payload paths')
        for member in archive.getmembers():
            parts = Path(member.name).parts
            require(not member.name.startswith('/') and '..' not in parts, 'unsafe payload path')
            require(not any(p in parts for p in ('var', 'home', 'root')), 'state in package payload')
        for name in binaries:
            member = archive.getmember(name)
            require(member.isfile(), 'regular packaged executable required')
            content = archive.extractfile(member).read()
            require(content[:6] == b'\x7fELF\x02\x01' and struct.unpack('<H', content[18:20])[0] == 62, 'wrong ELF architecture')
            with tempfile.TemporaryDirectory() as directory:
                binary = Path(directory) / 'binary'
                binary.write_bytes(content)
                require(embedded(binary) == expected, 'packaged binary identity mismatch')


def inspect_app(app, identity, production=False):
    app = Path(app)
    require(app.is_dir() and not app.is_symlink(), 'regular app bundle required')
    info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
    require(info['CFBundleIdentifier'] == 'com.botstral.desktop', 'desktop identifier changed')
    native = identity.native_versions()
    require(info['CFBundleVersion'] == native['macos_bundle'] and info['CFBundleShortVersionString'] == native['macos_short'], 'desktop native version mismatch')
    executable = app / 'Contents/MacOS/botstral-desktop'
    require(not executable.is_symlink(), 'regular desktop executable required')
    content = executable.read_bytes()
    require(content[:4] == b'\xcf\xfa\xed\xfe' and struct.unpack('<I', content[4:8])[0] == 0x0100000c, 'arm64 Mach-O required')
    require(embedded(executable)['identity'] == identity.json(), 'desktop binary identity mismatch')
    if production:
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True, capture_output=True)
        detail = subprocess.run(['codesign', '-d', '--verbose=4', str(app)], check=True, capture_output=True, text=True).stderr
        require('Authority=Developer ID Application:' in detail and 'TeamIdentifier=not set' not in detail, 'Developer ID Application signature required')
        subprocess.run(['spctl', '--assess', '--type', 'execute', '--verbose=2', str(app)], check=True, capture_output=True)
        subprocess.run(['xcrun', 'stapler', 'validate', str(app)], check=True, capture_output=True)
    return executable


def app_team(app):
    detail = subprocess.run(['codesign', '-d', '--verbose=4', str(app)], check=True, capture_output=True, text=True).stderr
    teams = [line.partition('=')[2] for line in detail.splitlines() if line.startswith('TeamIdentifier=')]
    require(len(teams) == 1 and teams[0] != 'not set', 'signed application TeamIdentifier required')
    return teams[0]


def mount_dmg(path, mount):
    subprocess.run(['hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', str(mount), str(path)], check=True, capture_output=True)


def inspect_artifacts(directory, release, production=False):
    for artifact in release.artifacts:
        path = Path(directory) / artifact.filename
        if artifact.filename.endswith('.deb'):
            inspect_deb(path, artifact, release)
        elif sys.platform == 'darwin':
            if production:
                subprocess.run(['codesign', '--verify', '--strict', str(path)], check=True, capture_output=True)
                subprocess.run(['xcrun', 'stapler', 'validate', str(path)], check=True, capture_output=True)
                subprocess.run(['spctl', '--assess', '--type', 'open', '--context', 'context:primary-signature', str(path)], check=True, capture_output=True)
            with tempfile.TemporaryDirectory() as directory:
                mount = Path(directory) / 'volume'
                mount_dmg(path, mount)
                try:
                    require({p.name for p in mount.iterdir()} == {'Botstral.app'}, 'unexpected DMG contents')
                    inspect_app(mount / 'Botstral.app', release.identity, production)
                finally:
                    subprocess.run(['hdiutil', 'detach', str(mount)], check=True, capture_output=True)
