#!/usr/bin/env python3
"""Check recovery coverage and the exact production Linux supervisor boundary."""
import argparse
import json
import os
from pathlib import Path
import socket
import ssl
import sqlite3
import subprocess
import sys
import uuid
from urllib.parse import urlsplit

from recovery import absolute, database, digest, locked, metadata, read_json, references, require, roots_for, safe_relative, under, walk, worker_records


def command(arguments):
    uid = os.geteuid()
    environment = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'XDG_RUNTIME_DIR': f'/run/user/{uid}', 'DBUS_SESSION_BUS_ADDRESS': f'unix:path=/run/user/{uid}/bus'}
    return subprocess.run(arguments, env=environment, capture_output=True, text=True, timeout=15, check=True).stdout


def supervisor():
    require(sys.platform == 'linux' and os.geteuid() != 0, 'nonroot Linux worker account required')
    require(Path('/proc/1/comm').read_text().strip() == 'systemd', 'systemd PID 1 required; ordinary OCI containers are unsupported')
    require(command(['/usr/bin/systemd-run', '--version']).startswith('systemd 255 '), 'production backend requires systemd 255')
    require(Path('/sys/fs/cgroup/cgroup.controllers').is_file(), 'cgroup v2 required')
    command(['/usr/bin/systemctl', '--user', 'show-environment'])
    require({'memory', 'pids'} <= set(Path('/sys/fs/cgroup/cgroup.controllers').read_text().split()), 'memory and pids controllers required')


def runtime_hashes(state):
    verified = []
    for name in ('computer.json', 'vibe.json'):
        path = state / name
        if not path.exists():
            continue
        configuration = read_json(path)
        root = absolute(configuration['directory'])
        manifest = absolute(configuration['manifest']) if name == 'vibe.json' else root / 'runtime.json'
        expected = configuration['runtime'] if name == 'vibe.json' else configuration['runtime']['sha256']
        require(digest(manifest) == expected, 'runtime manifest digest mismatch')
        value = read_json(manifest)
        if name == 'vibe.json':
            require(value['version'] == '2.24.2' and value['distributions']['mistral-vibe'] == '2.24.2', 'pinned Vibe version required')
            files = value['files'].items()
        else:
            files = value
        count = 0
        for relative, expected_digest in files:
            safe_relative(relative)
            require(relative != '.' and digest(absolute(root / relative)) == expected_digest, 'runtime file digest mismatch')
            count += 1
        require(count > 0, 'runtime manifest is empty')
        verified.append(name)
    return verified


def connectivity(configuration):
    endpoint = urlsplit(configuration['manager'])
    require(endpoint.scheme == 'wss' and endpoint.hostname and not endpoint.username and not endpoint.password and endpoint.path in {'', '/'} and not endpoint.query and not endpoint.fragment, 'invalid worker manager origin')
    for name in ('manager_ca', 'client_cert', 'client_key'):
        metadata(absolute(configuration[name]), True)
    context = ssl.create_default_context(cafile=configuration['manager_ca'])
    context.load_cert_chain(configuration['client_cert'], configuration['client_key'])
    with socket.create_connection((endpoint.hostname, endpoint.port or 443), timeout=5) as connection:
        with context.wrap_socket(connection, server_hostname=endpoint.hostname):
            pass


def diagnose(component, state, plan=None, connect=False, probe=False):
    state = absolute(state)
    if component == 'agent':
        supervisor()
    with locked(component, state):
        for _, _ in walk(state):
            pass
        connection, schema, identity = database(state / (component + '.sqlite3'), component)
        try:
            result = {'component': component, 'schema': schema, 'identity': identity, 'offline_lock': True}
            if component == 'agent':
                worker_records(connection)
                roots = roots_for(component, state, plan)
                refs = references(state)
                require(all(any(under(Path(ref), Path(root['path'])) for root in roots) for ref in refs), 'runtime or certificate not covered by recovery plan')
                for root in roots:
                    for _, _ in walk(Path(root['path'])):
                        pass
                result['runtime_hashes'] = runtime_hashes(state)
                result['root_count'] = len(roots)
                if probe:
                    scratch = state.parent / ('supervisor-check-' + uuid.uuid4().hex)
                    command(['/usr/bin/botstral-agent-supervisor-check', '--scratch', str(scratch)])
                    result['production_supervisor_proved'] = True
                if connect:
                    connectivity(read_json(state / 'config.json'))
                    result['tls_connected'] = True
            else:
                from recovery import artifact_check
                artifact_check(connection, state)
                result['vault_startup'] = 'locked'
                result['vault_decryption'] = 'not attempted; original owner passphrase required'
            return result
        finally:
            connection.close()


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser()
    parser.add_argument('component', choices=['manager', 'agent'])
    parser.add_argument('--state-dir', required=True)
    parser.add_argument('--plan')
    parser.add_argument('--connect', action='store_true')
    parser.add_argument('--probe-supervisor', action='store_true')
    args = parser.parse_args()
    try:
        result = diagnose(args.component, args.state_dir, read_json(absolute(args.plan)) if args.plan else None, args.connect, args.probe_supervisor)
        print(json.dumps(dict(result, ok=True), sort_keys=True))
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error, subprocess.SubprocessError) as error:
        print('diagnostic refused: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
