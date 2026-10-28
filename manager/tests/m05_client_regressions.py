#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid

import m05_acceptance as m


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binaries', type=Path, default=Path('target/debug'))
    parser.add_argument('--contract-bin', type=Path,
                        default=Path('target/debug/examples/m05-client-contract'))
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    args.output.mkdir(parents=True, exist_ok=False)
    root = Path(tempfile.mkdtemp(prefix='botstral-m05-client-'))
    fixture = proxy = None
    try:
        m.certificates(root)
        fixture = m.Fixture(root, args.binaries/'botstral-manager',
                            args.binaries/'owner-probe', args.binaries/'botstral-agent')
        fixture.start()
        fixture.setup()
        client, other = fixture.client('client'), fixture.client('other')
        bot, conversation = str(uuid.uuid4()), str(uuid.uuid4())
        fixture.owner('PUT', '/v1/bots/'+bot, {'name': 'Client contract'})
        fixture.owner('POST', '/v1/conversations', {'id': conversation, 'bot': bot})
        contract = m.command([args.contract_bin, client, conversation, other], timeout=20)
        (args.output/'client-contract.log').write_bytes(contract.stdout+contract.stderr)
        proxy = m.OpaqueProxy(fixture.addresses['public'], delay=0,
                              initial_alert=bytes.fromhex('15030300020250'))
        trust = dict(fixture.profile(), endpoint=proxy.address)
        m.write(root/'proxy-trust.json', trust)
        m.command([fixture.probe, 'trust', client, root/'proxy-trust.json'])
        watched = subprocess.run([fixture.probe, 'watch', client, '3'],
                                 capture_output=True, timeout=8)
        (args.output/'tls-retry.log').write_bytes(watched.stdout+watched.stderr)
        m.require(watched.returncode == 0 and proxy.accepted >= 2,
                  'Transient TLS alert stopped reconnect.')
        m.require((client/'state.json').exists(), 'Healthy retry did not install state.')
        checks = ['exclusive_client_storage_and_unknown_committed_mutation',
                  'transient_tls_alert_then_healthy_pinned_recovery']
        m.write(args.output/'summary.json', {'passed': True, 'checks': checks})
        print(json.dumps({'passed': True, 'checks': checks}))
    finally:
        if proxy:
            proxy.close()
        if fixture:
            fixture.stop()
        shutil.rmtree(root)


if __name__ == '__main__':
    main()
