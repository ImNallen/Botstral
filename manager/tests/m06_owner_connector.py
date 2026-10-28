#!/usr/bin/env python3
import argparse
import hashlib
import http.client
import json
from pathlib import Path
import signal
import sqlite3
import subprocess
import tempfile
import time
import uuid


def request(address, method, path, body=None, token=None, expected=200):
    host, port = address.rsplit(':', 1)
    connection = http.client.HTTPConnection(host, int(port), timeout=30)
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    connection.request(method, path, json.dumps(body) if body is not None else None, headers)
    response = connection.getresponse()
    data = response.read()
    connection.close()
    assert response.status == expected, (method, path, response.status, data.decode())
    return json.loads(data) if data else None


def exercise(binary, report):
    with tempfile.TemporaryDirectory(prefix='botstral-m06-owner-') as directory:
        root = Path(directory)
        state = root / 'state'
        log = root / 'manager.log'
        password, passphrase = 'synthetic-owner-password', 'synthetic-vault-passphrase'
        def start():
            stream = log.open('ab')
            child = subprocess.Popen([str(binary), '--state-dir', str(state), '--listen', '127.0.0.1:0', '--setup-listen', '127.0.0.1:0'], stdout=stream, stderr=stream)
            stream.close()
            for _ in range(200):
                if child.poll() is not None:
                    raise AssertionError(log.read_text())
                if (state / 'listeners.json').exists():
                    return child, json.loads((state / 'listeners.json').read_text())
                time.sleep(.05)
            child.kill()
            raise AssertionError('manager startup deadline')
        def stop(child):
            child.send_signal(signal.SIGTERM)
            assert child.wait(timeout=20) == 0, log.read_text()
        child, addresses = start()
        try:
            bootstrap = (state / 'bootstrap.token').read_text()
            request(addresses['setup'], 'POST', '/v1/owner', {'name': 'Connector fixture', 'password': password, 'vault_passphrase': passphrase, 'acknowledge_no_escrow': True}, bootstrap, 201)
            address = addresses['listen']
            token = request(address, 'POST', '/v1/sessions', {'password': password}, expected=201)['token']
            request(address, 'POST', '/v1/vault/unlock', {'passphrase': passphrase}, token, 204)
            identity = str(uuid.uuid4())
            body = {'id': identity, 'label': 'Synthetic account', 'transport': {'type': 'remote_http', 'endpoint': 'https://example.com/mcp'}}
            installed = request(address, 'POST', '/v1/connectors', body, token)
            assert request(address, 'POST', '/v1/connectors', body, token) == installed
            request(address, 'GET', '/v1/connectors', expected=401)
            invalid = dict(body, id=str(uuid.uuid4()), transport={'type': 'remote_http', 'endpoint': 'https://user:token@example.com/mcp?token=secret'})
            request(address, 'POST', '/v1/connectors', invalid, token, 400)
            secret = 'synthetic-connector-bearer-never-in-views'
            rotated = request(address, 'PUT', f'/v1/connectors/{identity}/secret', {'expected_revision': 1, 'secret': {'type': 'bearer', 'value': secret}}, token)
            assert rotated['version']['credential_epoch'] == 2
            assert secret not in json.dumps(request(address, 'GET', '/v1/connectors', token=token))
            bot = str(uuid.uuid4())
            request(address, 'PUT', '/v1/bots/' + bot, {'name': 'Synthetic bot'}, token)
            assert request(address, 'GET', '/v1/bots/' + bot + '/connector-tools', token=token) == []
            request(address, 'PUT', f'/v1/connectors/{identity}/secret', {'expected_revision': 1, 'secret': {'type': 'none'}}, token, 409)
            stop(child)
            child, addresses = start()
            address = addresses['listen']
            token = request(address, 'POST', '/v1/sessions', {'password': password}, expected=201)['token']
            retained = request(address, 'GET', '/v1/connectors/' + identity, token=token)
            assert retained == rotated
            request(address, 'POST', f'/v1/connectors/{identity}/reconnect', {'expected_revision': 2}, token, 409)
            request(address, 'POST', '/v1/vault/unlock', {'passphrase': passphrase}, token, 204)
            reconnected = request(address, 'POST', f'/v1/connectors/{identity}/reconnect', {'expected_revision': 2}, token)
            assert reconnected['version']['credential_epoch'] == 3
            revoked = request(address, 'POST', f'/v1/connectors/{identity}/revoke', {'expected_revision': 3}, token)
            assert revoked['availability'] == 'revoked'
            audit = request(address, 'GET', f'/v1/connectors/{identity}/audit/0', token=token)
            assert {event['code'] for event in audit} == {'installed', 'credential_rotated', 'reconnected', 'revoked'}
            assert secret not in json.dumps(audit)
            stop(child)
            child = None
            db = sqlite3.connect(state / 'manager.sqlite3')
            assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
            ciphertexts = db.execute('SELECT ciphertext FROM connection_secrets').fetchall()
            assert len(ciphertexts) == 3 and all(secret.encode() not in item[0] for item in ciphertexts)
            db.close()
            assert secret not in log.read_text()
            result = {'passed': True, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                      'checks': ['actual manager binary', 'protected install and secret entry', 'no implicit bot grants',
                                 'immutable rotation', 'locked restart', 'reconnect', 'revocation', 'encrypted SQLite', 'redacted audit'],
                      'external_services': False}
            report.write_text(json.dumps(result, indent=2) + '\n')
            print(json.dumps(result))
        finally:
            if child and child.poll() is None:
                stop(child)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--manager', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    exercise(args.manager.resolve(), args.report)
