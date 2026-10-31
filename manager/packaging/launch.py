#!/usr/bin/env python3
import argparse
import ipaddress
import os
from pathlib import Path
import sys


def arguments(path, binary='/usr/bin/botstral-manager'):
    from recovery import absolute, metadata, read_json
    path = absolute(path)
    metadata(path, True)
    value = read_json(path)
    required = {'state_dir', 'listen', 'setup_listen', 'public_listen'}
    if type(value) is not dict or set(value) != required or any(type(item) is not str for item in value.values()):
        raise ValueError('invalid manager service configuration')
    for key in ('listen', 'setup_listen', 'public_listen'):
        host, port = value[key].rsplit(':', 1)
        address = ipaddress.ip_address(host.strip('[]'))
        if not 1 <= int(port) <= 65535 or (key != 'public_listen' and not address.is_loopback):
            raise ValueError('invalid manager listener')
    state = absolute(value['state_dir'])
    metadata(state, True)
    pairs = [('state-dir', value['state_dir']), ('listen', value['listen']), ('setup-listen', value['setup_listen']), ('public-listen', value['public_listen'])]
    return [binary, *[argument for name, content in pairs for argument in ('--' + name, content)]]


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    args = parser.parse_args()
    try:
        command = arguments(Path(args.config))
        os.execv(command[0], command)
    except (OSError, ValueError, KeyError, TypeError):
        print('manager service configuration refused', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
