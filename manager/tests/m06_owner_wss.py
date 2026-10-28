#!/usr/bin/env python3
"""Exercise connector administration over the actual owner WSS transport."""
import argparse
import hashlib
import json
from pathlib import Path
import secrets
import tempfile
import uuid
from m05_acceptance import Fixture, WebSocket, certificates


def exercise(manager, report):
    with tempfile.TemporaryDirectory(prefix='botstral-m06-owner-wss-') as directory:
        root = Path(directory)
        certificates(root)
        fixture = Fixture(root, manager, None, None)
        fixture.start()
        websocket = None
        try:
            fixture.setup()
            fixture.owner('POST', '/v1/vault/unlock', {'passphrase': 'synthetic M05 vault passphrase'}, status=204)
            credential = secrets.token_urlsafe(32)
            fingerprint = hashlib.sha256(b'botstral-owner-device-v1\0' + credential.encode()).hexdigest()
            invitation = fixture.owner('POST', '/v1/device-invitations', {'request': {'fingerprint': fingerprint, 'label': 'M06 connector fixture'}, 'lifetime_seconds': 600})
            trust = fixture.profile()
            websocket = WebSocket(fixture.addresses['public'], fixture.context)
            websocket.send({'type': 'enroll', 'version': 2, 'manager': trust['manager'], 'role': 'owner',
                            'credential': credential, 'invitation': invitation['invitation']})
            assert websocket.receive()['type'] == 'welcome'
            identity = str(uuid.uuid4())
            installed = websocket.call({'method': 'install_connection', 'input': {'id': identity, 'label': 'Remote fixture',
                                        'transport': {'type': 'remote_http', 'endpoint': 'https://example.com/mcp'}}})['value']
            assert installed == fixture.owner('GET', '/v1/connectors/' + identity)
            secret = 'synthetic-wss-connector-secret'
            rotated = websocket.call({'method': 'replace_connection_secret', 'id': identity,
                                      'input': {'expected_revision': 1, 'secret': {'type': 'bearer', 'value': secret}}})['value']
            assert rotated['version']['credential_epoch'] == 2 and secret not in json.dumps(rotated)
            assert websocket.call({'method': 'connection', 'id': identity})['value'] == rotated
            assert websocket.call({'method': 'connections'})['value'] == [rotated]
            reconnected = websocket.call({'method': 'reconnect_connection', 'id': identity, 'expected_revision': 2})['value']
            assert reconnected['version']['revision'] == 3
            revoked = websocket.call({'method': 'revoke_connection', 'id': identity, 'expected_revision': 3})['value']
            assert revoked['availability'] == 'revoked'
            audit = websocket.call({'method': 'connector_audit', 'connection': identity, 'after': 0})['value']
            assert len(audit) == 4 and secret not in json.dumps(audit)
            snapshot = websocket.call({'method': 'workspace'})['value']['state']
            assert snapshot['connections'][0]['availability'] == 'revoked'
            result = {'passed': True, 'binary_sha256': hashlib.sha256(manager.read_bytes()).hexdigest(),
                      'owner_protocol': 2, 'checks': ['actual TLS WebSocket enrollment', 'install and list', 'protected secret entry',
                      'local API parity', 'reconnect', 'revocation', 'redacted audit', 'snapshot summary']}
            report.write_text(json.dumps(result, indent=2) + '\n')
            print(json.dumps(result))
        finally:
            if websocket:
                websocket.close()
            fixture.stop()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--manager', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    exercise(args.manager.resolve(), args.report)
