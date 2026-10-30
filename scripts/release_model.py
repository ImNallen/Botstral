"""Release metadata and validation. Python 3.9 and the OpenSSL command are sufficient."""
import dataclasses
from enum import Enum
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
MARKER = b'BOTSTRAL_RELEASE_V1\0'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_json(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'duplicate JSON key: ' + key)
            result[key] = value
        return result
    return json.loads(Path(path).read_text(), object_pairs_hook=unique)


def write_json(path, value):
    Path(path).write_text(json.dumps(value, sort_keys=True, indent=2) + '\n')


def authority():
    return read_json(ROOT / 'release.json')


def digest(path):
    with Path(path).open('rb') as stream:
        result = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


class DistributionChannel(str, Enum):
    STABLE = 'stable'
    NIGHTLY = 'nightly'


@dataclasses.dataclass(frozen=True)
class ReleaseIdentity:
    version: str
    channel: DistributionChannel
    revision: str
    build_id: str

    @classmethod
    def parse(cls, value):
        require(set(value) == {'version', 'channel', 'revision', 'build_id'}, 'invalid identity fields')
        match = re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-nightly\.(\d{8})\.([1-9][0-9]*))?', value['version'])
        require(match is not None, 'invalid release version')
        channel = DistributionChannel(value['channel'])
        require((channel == DistributionChannel.NIGHTLY) == (match[4] is not None), 'version/channel mismatch')
        require(re.fullmatch(r'[0-9a-f]{40}', value['revision']) is not None, 'full source revision required')
        require(re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,79}', value['build_id']) is not None, 'invalid build identifier')
        return cls(value['version'], channel, value['revision'], value['build_id'])

    def json(self):
        return dataclasses.asdict(self)

    def native_versions(self):
        base, _, prerelease = self.version.partition('-')
        if prerelease:
            _, _, sequence = prerelease.split('.')
            require(int(sequence) <= 255, 'nightly sequence exceeds CFBundleVersion development range')
            mac = base + 'd' + sequence
        else:
            mac = base
        return {'debian': self.version.replace('-nightly.', '~nightly.') + '-1',
                'macos_short': base, 'macos_bundle': mac}


def embedded(path):
    content = Path(path).read_bytes()
    found = set()
    offset = 0
    while True:
        offset = content.find(MARKER, offset)
        if offset < 0:
            break
        start = offset + len(MARKER)
        end = content.find(b'\0', start)
        require(end > start, 'unterminated binary release identity')
        found.add(content[start:end])
        offset = end + 1
    require(len(found) == 1, 'missing or mixed binary release identities')
    value = json.loads(found.pop())
    ReleaseIdentity.parse(value['identity'])
    return value


def filename(entry, identity):
    suffix = '_amd64.deb' if entry['kind'] == 'deb' else '_arm64.dmg'
    return 'botstral-' + entry['component'] + '_' + identity.version + suffix


@dataclasses.dataclass(frozen=True)
class Artifact:
    component: str
    target: str
    filename: str
    size: int
    sha256: str
    native_versions: dict
    signing: str
    identity: dict

    @classmethod
    def parse(cls, value, identity, registry):
        require(set(value) == {f.name for f in dataclasses.fields(cls)}, 'invalid artifact fields')
        entry = next((e for e in registry if e['component'] == value['component'] and e['target'] == value['target']), None)
        require(entry is not None, 'unexpected artifact or wrong target')
        require(value['filename'] == filename(entry, identity), 'unexpected or unsafe artifact filename')
        require(type(value['size']) is int and value['size'] > 0, 'invalid artifact size')
        require(re.fullmatch('[0-9a-f]{64}', value['sha256']) is not None, 'invalid artifact digest')
        require(value['identity'] == identity.json(), 'mixed artifact identity')
        require(value['native_versions'] == identity.native_versions(), 'native version mismatch')
        require(value['signing'] in ('unsigned', 'manifest', 'developer-id-notarized'), 'invalid signing evidence')
        return cls(**value)


@dataclasses.dataclass(frozen=True)
class ReleaseManifest:
    identity: ReleaseIdentity
    artifacts: tuple
    value: dict

    @classmethod
    def parse(cls, value, production=False):
        config = authority()
        require(set(value) == {'format', 'identity', 'protocols', 'link_schema_sha256', 'data_compatibility', 'runtime_requirements', 'artifacts', 'acceptance'}, 'invalid manifest fields')
        require(value['format'] == 1, 'unsupported manifest format')
        identity = ReleaseIdentity.parse(value['identity'])
        require(value['protocols'] == config['protocols'], 'protocol evidence mismatch')
        require(value['link_schema_sha256'] == digest(ROOT / 'link/schema/protocol.json'), 'link schema mismatch')
        for key in ('data_compatibility', 'runtime_requirements'):
            require(value[key] == config[key], key + ' mismatch')
        artifacts = tuple(Artifact.parse(a, identity, config['artifacts']) for a in value['artifacts'])
        require(sorted((a.component, a.target) for a in artifacts) == sorted((e['component'], e['target']) for e in config['artifacts']), 'missing or duplicate required artifacts')
        require(isinstance(value['acceptance'], list), 'invalid acceptance evidence')
        if production:
            expected = {'ubuntu24.04-amd64', 'macos-arm64'}
            receipts = value['acceptance']
            require(len(receipts) == len(expected) and {r['target'] for r in receipts} == expected, 'complete native acceptance required')
            for receipt in receipts:
                require(receipt['identity'] == identity.json() and receipt['passed'] is True and receipt['production'] is True, 'native acceptance did not pass production mode')
                require(receipt['artifacts'] == {a.filename: a.sha256 for a in artifacts if a.target == receipt['target']}, 'acceptance artifact mismatch')
            for a in artifacts:
                require(a.signing == ('developer-id-notarized' if a.component == 'desktop' else 'manifest'), 'production signing required')
        return cls(identity, artifacts, value)


def verify(directory, trusted_key=None, unsigned=False, production=False):
    directory = Path(directory)
    require(directory.is_dir() and not directory.is_symlink(), 'regular release directory required')
    manifest = directory / 'manifest.json'
    require(manifest.is_file() and not manifest.is_symlink(), 'regular manifest required')
    require(not (unsigned and production), 'unsigned mode cannot verify production')
    if not unsigned:
        require(trusted_key is not None, 'independently supplied trusted public key required')
        key = Path(trusted_key).resolve()
        require(directory.resolve() not in key.parents, 'trust key must be supplied outside release set')
        signature = directory / 'manifest.sig'
        require(signature.is_file() and not signature.is_symlink(), 'manifest signature required')
        subprocess.run(['openssl', 'dgst', '-sha256', '-verify', str(key), '-signature', str(signature), str(manifest)], check=True, capture_output=True)
    release = ReleaseManifest.parse(read_json(manifest), production)
    allowed = {'manifest.json', 'manifest.sig'} | {a.filename for a in release.artifacts}
    require({p.name for p in directory.iterdir()} <= allowed, 'unexpected release set files')
    for a in release.artifacts:
        path = directory / a.filename
        require(path.is_file() and not path.is_symlink(), 'missing or unsafe artifact')
        require(path.stat().st_size == a.size and digest(path) == a.sha256, 'corrupt artifact: ' + a.filename)
    return release
