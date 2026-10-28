import argparse
import contextlib
from dataclasses import asdict, dataclass
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import stat
import subprocess
import sys
import tempfile
import uuid

FORMAT = 1
POLICY = {
    'manager': 'offline-sqlite-snapshot;all-root-files;ledger-blobs;exclude-lock-wal-shm-parts',
    'agent': 'offline-sqlite-snapshot;identity-journal-process-evidence;explicit-guest-roots;selected-tool-bytes;no-clone',
}
EXCLUSIONS = {
    'manager': ['manager.lock', 'manager.sqlite3-wal', 'manager.sqlite3-shm', 'artifacts/*.part'],
    'agent': ['agent.lock', 'agent.sqlite3-wal', 'agent.sqlite3-shm', 'unselected guest roots', 'operating system and unselected tools', 'host and VM configuration'],
}
KINDS = {'state', 'workspace', 'browser-profile', 'coding-home', 'certificate', 'runtime', 'tool'}
MAX_MANIFEST = 16 * 1024 * 1024


@dataclass(frozen=True)
class BackupManifest:
    format: int
    backup_id: str
    component: str
    identity: str
    schema: int
    created_at: str
    policy: str
    exclusions: list
    roots: list
    entries: list
    database: str
    references: list
    recovery: str


def require(condition, message):
    if not condition:
        raise ValueError(message)


def absolute(path):
    path = Path(path)
    require(path.is_absolute() and '..' not in path.parts and str(path) != '/', 'absolute non-root path required')
    current = Path('/')
    for part in path.parts[1:]:
        current /= part
        try:
            metadata = current.lstat()
        except FileNotFoundError:
            continue
        require(not stat.S_ISLNK(metadata.st_mode), 'symlink in path')
    return path


def metadata(path, private=False):
    value = path.lstat()
    require(value.st_uid == os.geteuid(), 'path must be owned by maintenance user')
    require(stat.S_ISDIR(value.st_mode) or stat.S_ISREG(value.st_mode), 'regular file or directory required')
    require(not stat.S_ISREG(value.st_mode) or value.st_nlink == 1, 'hardlink refused')
    require(value.st_mode & 0o7000 == 0, 'special file mode refused')
    if private:
        require(stat.S_IMODE(value.st_mode) == (0o700 if path.is_dir() else 0o600), 'private permissions required')
    return value


def regular(path):
    before = metadata(path)
    require(stat.S_ISREG(before.st_mode), 'regular file required')
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    after = os.fstat(descriptor)
    if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
        os.close(descriptor)
        raise ValueError('file changed while opening')
    return os.fdopen(descriptor, 'rb')


def digest(path):
    with regular(path) as stream:
        result = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
        return result.hexdigest()


def read_json(path):
    with regular(path) as stream:
        data = stream.read(MAX_MANIFEST + 1)
    require(len(data) <= MAX_MANIFEST, 'JSON size bound exceeded')
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON field')
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique)


def write_json(path, value):
    with open(path, 'x', opener=lambda p, flags: os.open(p, flags | os.O_NOFOLLOW, 0o600)) as stream:
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())


def sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


@contextlib.contextmanager
def locked(component, state):
    state = absolute(state)
    metadata(state, True)
    lock = state / (component + '.lock')
    metadata(lock, True)
    descriptor = os.open(lock, os.O_RDWR | os.O_NOFOLLOW)
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield state
    finally:
        os.close(descriptor)


def database(path, component, immutable=False):
    with regular(path):
        pass
    connection = sqlite3.connect(path.as_uri() + ('?mode=ro&immutable=1' if immutable else '?mode=rw'), uri=True, timeout=0)
    try:
        require(connection.execute('PRAGMA integrity_check').fetchall() == [('ok',)], 'SQLite integrity failed')
        version = connection.execute('PRAGMA user_version').fetchone()[0]
        require(1 <= version <= (10 if component == 'manager' else 2), 'unsupported database schema')
        if component == 'manager':
            row = connection.execute('SELECT identity_ciphertext FROM vault WHERE id=1').fetchone()
            require(row is not None, 'configured vault required')
            identity = hashlib.sha256(row[0]).hexdigest()
        else:
            identity = connection.execute('SELECT incarnation FROM metadata').fetchone()[0]
            require(str(uuid.UUID(identity)) == identity, 'invalid worker identity')
        return connection, version, identity
    except BaseException:
        connection.close()
        raise


def artifact_check(connection, root):
    for identity, offer in connection.execute("SELECT id,offer FROM artifacts WHERE state='available'"):
        require(str(uuid.UUID(identity)) == identity, 'invalid artifact identity')
        content = json.loads(offer)['output']
        require(content['type'] == 'content', 'available artifact lacks content')
        content = content['content']
        path = root / 'artifacts' / (identity + '.blob')
        require(metadata(path).st_size == content['length'] and digest(path) == content['sha256'], 'ledger artifact integrity failed')


def worker_records(connection):
    for (record,) in connection.execute('SELECT record FROM executions'):
        process = json.loads(record).get('process')
        require(process is None or process['lifecycle']['type'] == 'stopped', 'worker process stop is unproved')


def under(path, root):
    return path == root or root in path.parents


def worker_quiet(roots):
    require(sys.platform == 'linux', 'worker quiescence requires Linux procfs and systemd')
    uid = os.geteuid()
    environment = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'XDG_RUNTIME_DIR': f'/run/user/{uid}', 'DBUS_SESSION_BUS_ADDRESS': f'unix:path=/run/user/{uid}/bus'}
    result = subprocess.run(['/usr/bin/systemctl', '--user', 'list-units', '--all', '--no-legend', '--plain', 'botstral-*.service'], env=environment, capture_output=True, timeout=10, check=True, text=True)
    require(all(len(line.split()) >= 4 and line.split()[2] in {'inactive', 'failed'} for line in result.stdout.splitlines() if line.strip()), 'worker systemd units still active')
    require(Path('/proc/self/fd').is_dir() and Path('/sys/fs/cgroup/cgroup.controllers').is_file(), 'procfs and cgroup v2 required')
    manager_pid = subprocess.run(['/usr/bin/systemctl', 'show', f'user@{uid}.service', '--property=MainPID', '--value'], env=environment, capture_output=True, timeout=10, check=True, text=True).stdout.strip()
    require(manager_pid.isdecimal() and int(manager_pid) > 0, 'systemd user manager identity unavailable')
    manager_group = Path(f'/proc/{manager_pid}/cgroup').read_text()
    ancestors = {os.getpid(), int(manager_pid)}
    parent = os.getppid()
    while parent and parent not in ancestors:
        ancestors.add(parent)
        try:
            parent = int(Path(f'/proc/{parent}/stat').read_text().rsplit(')', 1)[1].split()[1])
        except FileNotFoundError:
            break
    for process in Path('/proc').iterdir():
        if not process.name.isdigit() or int(process.name) in ancestors:
            continue
        try:
            if process.stat().st_uid != uid:
                continue
            name = (process / 'comm').read_text().strip().lower()
            if name == '(sd-pam)':
                status = (process / 'status').read_text()
                parent_line = next((line.split()[1] for line in status.splitlines() if line.startswith('PPid:')), None)
                if parent_line == manager_pid and (process / 'cgroup').read_text() == manager_group:
                    continue
            require(not any(word in name for word in ('chrom', 'firefox', 'vibe', 'botstral')), 'guest browser, coding, or worker process still running')
            links = [process / 'cwd', process / 'exe', *list((process / 'fd').iterdir())]
            for link in links:
                try:
                    target = Path(os.readlink(link).removesuffix(' (deleted)'))
                except FileNotFoundError:
                    continue
                require(not target.is_absolute() or not any(under(target, root) for root in roots), 'process still holds a selected root')
        except (FileNotFoundError, ProcessLookupError):
            continue


def references(state):
    result = []
    config = read_json(state / 'config.json')
    for name in ('manager_ca', 'client_cert', 'client_key'):
        result.append(str(absolute(config[name])))
    for name in ('computer.json', 'vibe.json', 'connector.json'):
        path = state / name
        if path.exists():
            config = read_json(path)
            for key in ('directory', 'manifest', 'python'):
                if key in config:
                    result.append(str(absolute(config[key])))
    return sorted(set(result))


def roots_for(component, state, plan):
    roots = [{'label': 'state', 'kind': 'state', 'path': str(state)}]
    if component == 'agent':
        require(plan is not None, 'explicit worker recovery plan required')
        require(type(plan) is dict and set(plan) == {'roots'}, 'invalid recovery plan')
        for root in plan['roots']:
            require(type(root) is dict and set(root) == {'label', 'kind', 'path'}, 'invalid recovery root')
            require(root['kind'] in KINDS - {'state'}, 'invalid guest root kind')
            roots.append(dict(root, path=str(absolute(root['path']))))
    validate_roots(roots, component)
    return roots


def validate_roots(roots, component):
    require(type(roots) is list and 1 <= len(roots) <= 128, 'invalid root count')
    labels = set()
    paths = []
    for index, root in enumerate(roots):
        require(type(root) is dict and set(root) == {'label', 'kind', 'path'}, 'invalid root fields')
        require(type(root['label']) is str and re.fullmatch('[a-z][a-z0-9-]{0,63}', root['label']) and root['label'] not in labels, 'invalid root label')
        require(root['kind'] in KINDS and (root['kind'] == 'state') == (index == 0) and (root['label'] == 'state') == (index == 0), 'invalid root kind')
        path = absolute(root['path'])
        require(not any(under(path, other) or under(other, path) for other in paths), 'overlapping recovery roots')
        labels.add(root['label'])
        paths.append(path)
    require(component == 'agent' or len(roots) == 1, 'manager guest roots refused')


def walk(root, relative='.'):
    path = root if relative == '.' else root / relative
    info = metadata(path)
    yield relative, info
    if stat.S_ISDIR(info.st_mode):
        for child in sorted(path.iterdir()):
            yield from walk(root, child.name if relative == '.' else relative + '/' + child.name)


def excluded(component, label, relative):
    return label == 'state' and (relative in {component + '.lock', component + '.sqlite3-wal', component + '.sqlite3-shm'} or (component == 'manager' and relative.startswith('artifacts/') and relative.endswith('.part')))


def manager_layout(relative, info):
    if relative == '.':
        return
    parts = relative.split('/')
    require((len(parts) == 1 and (stat.S_ISREG(info.st_mode) or (relative == 'artifacts' and stat.S_ISDIR(info.st_mode)))) or (len(parts) == 2 and parts[0] == 'artifacts' and stat.S_ISREG(info.st_mode)), 'unsupported manager state layout')


def copy_file(source, destination):
    with regular(source) as src, open(destination, 'xb', opener=lambda p, flags: os.open(p, flags | os.O_NOFOLLOW, 0o600)) as dst:
        shutil.copyfileobj(src, dst, 1024 * 1024)
        dst.flush()
        os.fsync(dst.fileno())


def entry(label, relative, info, path):
    directory = stat.S_ISDIR(info.st_mode)
    return {'root': label, 'path': relative, 'kind': 'directory' if directory else 'file', 'mode': stat.S_IMODE(info.st_mode), 'size': 0 if directory else info.st_size, 'sha256': None if directory else digest(path)}


def create(component, state, destination, plan=None):
    destination = absolute(destination)
    metadata(destination.parent, True)
    require(not destination.exists(), 'backup destination must be new')
    with locked(component, state) as state:
        roots = roots_for(component, state, plan)
        require(not any(under(destination, Path(root['path'])) for root in roots), 'backup cannot be inside selected roots')
        refs = references(state) if component == 'agent' else []
        require(all(any(under(Path(ref), Path(root['path'])) for root in roots) for ref in refs), 'certificate or runtime reference is not covered')
        with contextlib.ExitStack() as stack:
            connection, schema, identity = database(state / (component + '.sqlite3'), component)
            stack.callback(connection.close)
            if component == 'manager':
                artifact_check(connection, state)
            else:
                worker_records(connection)
                worker_quiet([Path(root['path']) for root in roots])
            temporary = Path(tempfile.mkdtemp(prefix='.backup-', dir=destination.parent))
            try:
                payload = temporary / 'files'
                payload.mkdir(mode=0o700)
                entries = []
                for root in roots:
                    source = Path(root['path'])
                    require(stat.S_ISDIR(metadata(source).st_mode), 'recovery roots must be directories')
                    target = payload / root['label']
                    inventory = list(walk(source))
                    if root['label'] == 'state':
                        require(all(stat.S_IMODE(info.st_mode) == (0o700 if stat.S_ISDIR(info.st_mode) else 0o600) for _, info in inventory), 'state permissions must be private')
                    for relative, info in inventory:
                        if component == 'manager':
                            manager_layout(relative, info)
                        if excluded(component, root['label'], relative):
                            continue
                        out = target if relative == '.' else target / relative
                        src = source if relative == '.' else source / relative
                        if stat.S_ISDIR(info.st_mode):
                            out.mkdir(mode=0o700)
                        elif root['label'] == 'state' and relative == component + '.sqlite3':
                            snapshot = sqlite3.connect(out)
                            try:
                                connection.backup(snapshot)
                            finally:
                                snapshot.close()
                            out.chmod(0o600)
                            with regular(out) as stream:
                                os.fsync(stream.fileno())
                        else:
                            copy_file(src, out)
                        value = entry(root['label'], relative, info, out)
                        if value['kind'] == 'file':
                            value['size'] = out.stat().st_size
                        entries.append(value)
                    after = list(walk(source))
                    signature = lambda rows: [(rel, info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns) for rel, info in rows]
                    require(signature(inventory) == signature(after), 'source changed during backup')
                if component == 'agent':
                    worker_quiet([Path(root['path']) for root in roots])
                manifest = BackupManifest(FORMAT, str(uuid.uuid4()), component, identity, schema, datetime.datetime.now(datetime.timezone.utc).isoformat(), POLICY[component], EXCLUSIONS[component], roots, entries, component + '.sqlite3', refs, 'owner-passphrase-required;locked-startup' if component == 'manager' else 'original-instance-retired;retain-identity;never-clone;external-roots-at-original-paths')
                write_json(temporary / 'manifest.json', asdict(manifest))
                verify(component, temporary)
                for directory, _, _ in os.walk(temporary, topdown=False):
                    sync_directory(directory)
                os.rename(temporary, destination)
                sync_directory(destination.parent)
                return manifest.backup_id
            finally:
                if temporary.exists():
                    shutil.rmtree(temporary)


def safe_relative(value):
    require(type(value) is str and (value == '.' or (value and not value.startswith('/') and all(part not in {'', '.', '..'} for part in value.split('/')))), 'unsafe manifest path')


def verify(component, bundle):
    bundle = absolute(bundle)
    metadata(bundle, True)
    metadata(bundle / 'manifest.json', True)
    value = read_json(bundle / 'manifest.json')
    require(type(value) is dict and set(value) == set(BackupManifest.__dataclass_fields__), 'invalid manifest fields')
    manifest = BackupManifest(**value)
    require(type(manifest.format) is int and manifest.format == FORMAT and manifest.component == component, 'unsupported backup format or component')
    require(str(uuid.UUID(manifest.backup_id)) == manifest.backup_id, 'invalid backup identity')
    require(manifest.policy == POLICY[component] and manifest.exclusions == EXCLUSIONS[component], 'unsupported recovery policy')
    require(manifest.recovery == ('owner-passphrase-required;locked-startup' if component == 'manager' else 'original-instance-retired;retain-identity;never-clone;external-roots-at-original-paths'), 'invalid recovery requirement')
    require(manifest.database == component + '.sqlite3', 'invalid database path')
    validate_roots(manifest.roots, component)
    require(type(manifest.entries) is list and len(manifest.entries) <= 1000000, 'invalid entry count')
    labels = {root['label'] for root in manifest.roots}
    seen = set()
    for item in manifest.entries:
        require(type(item) is dict and set(item) == {'root', 'path', 'kind', 'mode', 'size', 'sha256'}, 'invalid entry fields')
        require(item['root'] in labels and item['kind'] in {'file', 'directory'}, 'invalid entry kind or root')
        safe_relative(item['path'])
        require(not excluded(component, item['root'], item['path']), 'excluded path in manifest')
        require(type(item['mode']) is int and 0 <= item['mode'] <= 0o777 and type(item['size']) is int and item['size'] >= 0, 'invalid entry mode or size')
        key = (item['root'], item['path'])
        require(key not in seen, 'duplicate manifest path')
        seen.add(key)
        path = bundle / 'files' / item['root']
        if item['path'] != '.':
            path /= item['path']
        info = metadata(absolute(path), True)
        if component == 'manager':
            manager_layout(item['path'], info)
        if item['kind'] == 'directory':
            require(stat.S_ISDIR(info.st_mode) and item['size'] == 0 and item['sha256'] is None, 'invalid directory entry')
        else:
            require(stat.S_ISREG(info.st_mode) and info.st_size == item['size'] and digest(path) == item['sha256'], 'backup file integrity failed')
        if item['root'] == 'state':
            require(item['mode'] == (0o700 if item['kind'] == 'directory' else 0o600), 'unsafe restored state mode')
    require(all((label, '.') in seen for label in labels), 'missing root entry')
    actual = {('manifest.json',), ('files',)}
    actual |= {('files', label, rel) for label in labels for rel, _ in walk(bundle / 'files' / label)}
    expected = {('manifest.json',), ('files',)} | {('files', label, rel) for label, rel in seen}
    require(actual == expected and {p.name for p in bundle.iterdir()} == {'files', 'manifest.json'} and {p.name for p in (bundle / 'files').iterdir()} == labels, 'unexpected backup content')
    connection, schema, identity = database(bundle / 'files/state' / manifest.database, component, immutable=True)
    try:
        require(type(manifest.schema) is int and schema == manifest.schema and identity == manifest.identity, 'database identity or schema mismatch')
        if component == 'manager':
            artifact_check(connection, bundle / 'files/state')
            require(manifest.references == [], 'unexpected manager reference')
        else:
            worker_records(connection)
            require(manifest.references == references(bundle / 'files/state'), 'worker references mismatch')
            require(all(any(under(Path(ref), Path(root['path'])) for root in manifest.roots) for ref in manifest.references), 'uncovered worker reference')
    finally:
        connection.close()
    return manifest


def restore(component, bundle, destination, retired=False):
    manifest = verify(component, bundle)
    require(component != 'agent' or retired, 'explicit --original-instance-retired required; never run two copies')
    with contextlib.ExitStack() as locks:
        original = Path(manifest.roots[0]['path'])
        if component == 'agent' and original.exists():
            locks.enter_context(locked('agent', original))
            connection, _, _ = database(original / 'agent.sqlite3', 'agent')
            try:
                worker_records(connection)
            finally:
                connection.close()
            worker_quiet([Path(root['path']) for root in manifest.roots])
        require(component != 'agent' or absolute(destination) == original, 'worker restore requires the original state path; configuration and process evidence contain absolute paths')
        return restore_validated(component, bundle, destination, manifest)


def restore_validated(component, bundle, destination, manifest):
    destinations = {root['label']: absolute(destination if root['label'] == 'state' else root['path']) for root in manifest.roots}
    for target in destinations.values():
        metadata(target.parent, True)
        require(not target.exists(), 'restore destination must be new')
        require(not under(absolute(bundle), target), 'restore overlaps backup')
    require(all(not under(a, b) for a in destinations.values() for b in destinations.values() if a != b), 'overlapping restore destinations')
    staged = {}
    published = []
    try:
        for label, target in destinations.items():
            staging = Path(tempfile.mkdtemp(prefix='.restore-', dir=target.parent))
            staged[label] = staging
            selected = [item for item in manifest.entries if item['root'] == label]
            root_entry = next(item for item in selected if item['path'] == '.')
            require(root_entry['kind'] == 'directory', 'restore roots must be directories')
            for item in sorted(selected, key=lambda row: (row['path'].count('/'), row['path'])):
                if item['path'] == '.':
                    continue
                source = Path(bundle) / 'files' / label / item['path']
                out = staging / item['path']
                if item['kind'] == 'directory':
                    out.mkdir(mode=0o700)
                else:
                    copy_file(source, out)
                    require(digest(out) == item['sha256'], 'backup changed during restore')
            for item in sorted(selected, key=lambda row: row['path'].count('/'), reverse=True):
                path = staging if item['path'] == '.' else staging / item['path']
                path.chmod(item['mode'])
                if item['kind'] == 'directory':
                    sync_directory(path)
        verify(component, bundle)
        for label, target in destinations.items():
            require(not target.exists(), 'restore destination appeared')
            os.rename(staged[label], target)
            published.append(str(target))
            sync_directory(target.parent)
        return manifest.backup_id
    except BaseException as error:
        if published:
            raise ValueError('partial restore published; keep services stopped: ' + ', '.join(published)) from error
        raise
    finally:
        for staging in staged.values():
            if staging.exists():
                shutil.rmtree(staging)


def main(component):
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=f'Offline {component} backup maintenance. Bundle contents are private plaintext except existing encrypted database fields.')
    commands = parser.add_subparsers(dest='command', required=True)
    create_parser = commands.add_parser('create')
    create_parser.add_argument('--state-dir', required=True)
    create_parser.add_argument('--destination', required=True)
    if component == 'agent':
        create_parser.add_argument('--plan', required=True)
    verify_parser = commands.add_parser('verify')
    verify_parser.add_argument('--backup', required=True)
    restore_parser = commands.add_parser('restore')
    restore_parser.add_argument('--backup', required=True)
    restore_parser.add_argument('--destination', required=True)
    if component == 'agent':
        restore_parser.add_argument('--original-instance-retired', action='store_true')
    args = parser.parse_args()
    try:
        if args.command == 'create':
            result = create(component, args.state_dir, args.destination, read_json(absolute(args.plan)) if component == 'agent' else None)
        elif args.command == 'verify':
            result = verify(component, args.backup).backup_id
        else:
            result = restore(component, args.backup, args.destination, getattr(args, 'original_instance_retired', False))
        print(json.dumps({'component': component, 'operation': args.command, 'backup_id': result, 'ok': True}))
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error, subprocess.SubprocessError) as error:
        print(f'{component} recovery refused: {error}', file=sys.stderr)
        return 1
    return 0
