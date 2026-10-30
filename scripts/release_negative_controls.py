#!/usr/bin/env python3
"""Exercise verifier refusals against a real built release set."""
import argparse
import copy
from pathlib import Path
import shutil
import tempfile

from release_model import require, verify, write_json
from release_artifacts import inspect_artifacts


def controls(directory, output):
    release = verify(directory, unsigned=True)
    inspect_artifacts(directory, release)
    results = []
    with tempfile.TemporaryDirectory(prefix='botstral-negative-') as temporary:
        root = Path(temporary) / 'release'
        shutil.copytree(directory, root)
        artifact = root / release.artifacts[0].filename
        original = artifact.read_bytes()
        artifact.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
        expect_refusal(root, 'corrupt artifact')
        results.append('corrupt artifact')
        artifact.unlink()
        expect_refusal(root, 'missing or unsafe artifact')
        results.append('missing artifact')
        artifact.write_bytes(original)
        for name, change in [('incomplete registry', lambda m: m['artifacts'].pop()),
                             ('mixed identity', lambda m: m['artifacts'][0]['identity'].update(build_id='different-build')),
                             ('wrong target', lambda m: m['artifacts'][0].update(target='unsupported-target')),
                             ('unsafe artifact path', lambda m: m['artifacts'][0].update(filename='../escape'))]:
            value = copy.deepcopy(release.value)
            change(value)
            write_json(root / 'manifest.json', value)
            expect_refusal(root)
            results.append(name)
    write_json(output, {'passed': True, 'identity': release.identity.json(), 'negative_controls': results})


def expect_refusal(directory, expected=None):
    try:
        verify(directory, unsigned=True)
    except ValueError as error:
        require(expected is None or expected in str(error), 'unexpected negative control error: ' + str(error))
    else:
        raise ValueError('negative control was accepted')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    controls(args.directory, args.output)
