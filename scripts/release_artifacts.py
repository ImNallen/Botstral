import io
import plistlib
from pathlib import Path
import struct
import subprocess
import sys
import tarfile
import tempfile

from release_model import embedded, require


def checked_command(args, **kwargs):
    try:
        return subprocess.run(args, check=True, capture_output=True, **kwargs)
    except subprocess.CalledProcessError as error:
        for stream, output in ((sys.stdout, error.stdout), (sys.stderr, error.stderr)):
            if output:
                stream.buffer.write(output.encode() if isinstance(output, str) else output)
                stream.flush()
        raise


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


def validate_payload(archive, component):
    package = 'botstral-' + component
    files = {'usr/bin/' + package, 'usr/bin/' + package + '-backup', 'usr/bin/' + package + '-diagnose',
             'usr/lib/systemd/' + ('system' if component == 'manager' else 'user') + '/' + package + '.service'}
    if component == 'agent':
        files.add('usr/bin/' + package + '-supervisor-check')
    roots = {'usr/lib/' + package, 'usr/share/' + package}
    directories = {str(parent) for name in files | roots for parent in Path(name).parents} | roots
    for member in archive.getmembers():
        name = member.name.removeprefix('./').rstrip('/')
        require(not member.name.startswith('/') and '..' not in Path(name).parts, 'unsafe payload path')
        require(member.uid == 0 and member.gid == 0 and not member.mode & 0o7000, 'unsafe payload ownership or mode')
        owned = any(name.startswith(root + '/') for root in roots)
        require(name in files or owned or (member.isdir() and name in directories), 'unexpected package payload path')
        if member.issym():
            require(name == 'usr/bin/' + package + '-backup' and member.linkname == '/usr/lib/' + package + '/backup.py', 'unsafe package symlink')
        else:
            require(member.isdir() or member.isfile(), 'unsafe package payload type')


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
        validate_payload(archive, artifact.component)
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
        checked_command(['codesign', '--verify', '--deep', '--strict', str(app)])
        detail = checked_command(['codesign', '-d', '--verbose=4', str(app)], text=True).stderr
        require('Authority=Developer ID Application:' in detail and 'TeamIdentifier=not set' not in detail, 'Developer ID Application signature required')
        checked_command(['spctl', '--assess', '--type', 'execute', '--verbose=2', str(app)])
        checked_command(['xcrun', 'stapler', 'validate', str(app)])
    return executable


def app_team(app):
    detail = checked_command(['codesign', '-d', '--verbose=4', str(app)], text=True).stderr
    teams = [line.partition('=')[2] for line in detail.splitlines() if line.startswith('TeamIdentifier=')]
    require(len(teams) == 1 and teams[0] != 'not set', 'signed application TeamIdentifier required')
    return teams[0]


def mount_dmg(path, mount):
    checked_command(['hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', str(mount), str(path)])


def inspect_artifacts(directory, release, production=False):
    for artifact in release.artifacts:
        path = Path(directory) / artifact.filename
        if artifact.filename.endswith('.deb'):
            inspect_deb(path, artifact, release)
        elif sys.platform == 'darwin':
            if production:
                checked_command(['codesign', '--verify', '--strict', str(path)])
                checked_command(['xcrun', 'stapler', 'validate', str(path)])
                checked_command(['spctl', '--assess', '--type', 'open', '--context', 'context:primary-signature', str(path)])
            with tempfile.TemporaryDirectory() as directory:
                mount = Path(directory) / 'volume'
                mount_dmg(path, mount)
                try:
                    require({p.name for p in mount.iterdir()} == {'Botstral.app'}, 'unexpected DMG contents')
                    inspect_app(mount / 'Botstral.app', release.identity, production)
                finally:
                    checked_command(['hdiutil', 'detach', str(mount)])
