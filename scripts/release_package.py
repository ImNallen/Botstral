import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile

from release_model import (ROOT, ReleaseIdentity, authority, digest, embedded, filename,
                           read_json, require, write_json)
from release_artifacts import checked_command, inspect_app


def command(args, **kwargs):
    argv = [str(a) for a in args]
    if (len(argv) == 4 and argv[0] == 'xcrun' and argv[1] == 'stapler'
            and argv[2] in ('validate', 'staple')):
        return checked_command(argv, **kwargs)
    subprocess.run(argv, check=True, **kwargs)


def verify_source():
    from release import versions
    mirrors = versions(mirrors_only=True)
    changed = subprocess.check_output(['git', 'diff', '--name-only', 'HEAD'], cwd=ROOT, text=True).splitlines()
    require(not subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard'], cwd=ROOT), 'untracked source files in production build')
    for name in changed:
        path = ROOT / name
        require(path in mirrors, 'production source differs from pinned revision: ' + name)
        original = subprocess.check_output(['git', 'show', 'HEAD:' + name], cwd=ROOT, text=True)
        normalized = mirrors[path]
        require(json.loads(original) == json.loads(normalized) if path.suffix == '.json' else original == normalized, 'production source mirror has unrelated edits: ' + name)


def build(identity_path, target, output, unsigned):
    from release import versions, validate_release_ref
    identity_path = Path(identity_path).resolve()
    identity = ReleaseIdentity.parse(read_json(identity_path))
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    require(os.environ.get('BOTSTRAL_SOURCE_REVISION', revision) == revision, 'source revision override differs from checkout')
    require(identity.revision == revision, 'source revision mismatch')
    versions(identity=identity)
    if not unsigned:
        verify_source()
        validate_release_ref(identity)
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, BOTSTRAL_RELEASE_IDENTITY=str(identity_path))
    entries = [e for e in authority()['artifacts'] if e['target'] == target]
    if target == 'ubuntu24.04-amd64':
        require(sys.platform == 'linux' and platform.machine() == 'x86_64', 'build Debian packages on Ubuntu amd64')
        require('VERSION_ID="24.04"' in Path('/etc/os-release').read_text(), 'Ubuntu 24.04 build host required')
        command(['cargo', 'build', '--release', '--locked', '--workspace'], cwd=ROOT, env=env)
        for entry in entries:
            binary = Path(env.get('CARGO_TARGET_DIR', str(ROOT / 'target'))) / 'release' / ('botstral-' + entry['component'])
            require(embedded(binary)['identity'] == identity.json(), 'built binary identity mismatch')
            package = build_deb(entry, binary, identity, output)
            write_descriptor(entry, package, identity, 'unsigned' if unsigned else 'manifest')
    else:
        require(sys.platform == 'darwin' and platform.machine() == 'arm64', 'build desktop on Apple silicon')
        package = build_dmg(identity, output, env, unsigned)
        write_descriptor(entries[0], package, identity, 'unsigned' if unsigned else 'developer-id-notarized')


def write_descriptor(entry, path, identity, signing):
    write_json(path.with_name(path.name + '.artifact.json'), {
        'component': entry['component'], 'target': entry['target'], 'filename': path.name,
        'size': path.stat().st_size, 'sha256': digest(path), 'native_versions': identity.native_versions(),
        'signing': signing, 'identity': identity.json()})


def build_deb(entry, binary, identity, output):
    component = entry['component']
    with tempfile.TemporaryDirectory(prefix='botstral-stage-') as directory:
        stage = Path(directory) / 'root'
        identity_file = Path(directory) / 'identity.json'
        write_json(identity_file, identity.json())
        command([sys.executable, ROOT / component / 'packaging/install.py', '--binary', binary, '--root', stage, '--identity', identity_file])
        config = stage / 'etc' / ('botstral-' + component)
        examples = stage / 'usr/share' / ('botstral-' + component)
        examples.parent.mkdir(parents=True)
        config.rename(examples)
        (stage / 'etc').rmdir()
        write_json(stage / 'usr/lib' / ('botstral-' + component) / 'release.json', identity.json())
        control = stage / 'DEBIAN'
        control.mkdir()
        native = identity.native_versions()['debian']
        text = (ROOT / component / 'packaging/debian/control').read_text().replace('@VERSION@', native)
        (control / 'control').write_text(text)
        guard = (ROOT / 'manager/packaging/debian/guard.py').read_text()
        for name, operation in [('preinst', 'install'), ('prerm', 'remove')]:
            arguments = json.dumps([component, native, identity.json(), operation])
            script = '#!/bin/sh\nset -eu\nexec /usr/bin/python3 - ' + "'" + arguments + "'" + ' "$@" <<\'BOTSTRAL_GUARD\'\n' + guard + '\nBOTSTRAL_GUARD\n'
            (control / name).write_text(script)
            (control / name).chmod(0o755)
        path = Path(output) / filename(entry, identity)
        command(['dpkg-deb', '--root-owner-group', '-Zxz', '--build', stage, path])
    return path


def build_dmg(identity, output, env, unsigned):
    signing = env.get('APPLE_SIGNING_IDENTITY')
    notarization = env.get('APPLE_NOTARY_PROFILE')
    require(unsigned or (signing and notarization), 'Developer ID signing identity and notarytool keychain profile required')
    command(['npm', 'ci'], cwd=ROOT, env=env)
    arguments = ['npm', 'run', 'desktop', '--', 'build', '--ci', '--no-sign', '--bundles', 'app']
    if env.get('TAURI_SIGNING_PRIVATE_KEY'):
        arguments.extend(['--config', json.dumps({'bundle': {'createUpdaterArtifacts': True}})])
    arguments.extend(['--', '--locked'])
    command(arguments, cwd=ROOT, env=env)
    bundle = ROOT / 'desktop/src-tauri/target/release/bundle/macos'
    updater = Path(output).parent / 'updater-bundle'
    for name in ('Botstral.app.tar.gz', 'Botstral.app.tar.gz.sig'):
        artifact = bundle / name
        if artifact.is_file() and not artifact.is_symlink():
            updater.mkdir(exist_ok=True)
            shutil.copy2(artifact, updater / name)
    if env.get('TAURI_SIGNING_PRIVATE_KEY') and not (updater / 'Botstral.app.tar.gz').is_file():
        require(False, 'updater signing key did not produce an updater bundle')
    source = bundle / 'Botstral.app'
    with tempfile.TemporaryDirectory(prefix='botstral-dmg-') as directory:
        stage = Path(directory) / 'volume'
        stage.mkdir()
        app = stage / 'Botstral.app'
        command(['ditto', source, app])
        import plistlib
        path = app / 'Contents/Info.plist'
        value = plistlib.loads(path.read_bytes())
        value['CFBundleShortVersionString'] = identity.native_versions()['macos_short']
        value['CFBundleVersion'] = identity.native_versions()['macos_bundle']
        path.write_bytes(plistlib.dumps(value))
        if unsigned:
            command(['codesign', '--force', '--deep', '--sign', signing or '-', app])
        else:
            command(['codesign', '--force', '--deep', '--options', 'runtime', '--timestamp', '--sign', signing, app])
            archive = Path(directory) / 'app.zip'
            command(['ditto', '-c', '-k', '--keepParent', app, archive])
            command(['xcrun', 'notarytool', 'submit', archive, '--keychain-profile', notarization, '--wait'])
            command(['xcrun', 'stapler', 'staple', app])
        inspect_app(app, identity, production=not unsigned)
        path = Path(output) / filename(next(e for e in authority()['artifacts'] if e['component'] == 'desktop'), identity)
        command(['hdiutil', 'create', '-volname', 'Botstral ' + identity.version, '-srcfolder', stage, '-format', 'UDZO', path])
        if not unsigned:
            command(['codesign', '--force', '--timestamp', '--sign', signing, path])
            command(['xcrun', 'notarytool', 'submit', path, '--keychain-profile', notarization, '--wait'])
            command(['xcrun', 'stapler', 'staple', path])
    return path
