#!/usr/bin/env python3
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid

module = importlib.util.spec_from_file_location('connector_runtime', Path(__file__).parents[1] / 'connector/runtime.py')
runtime = importlib.util.module_from_spec(module)
module.loader.exec_module(runtime)
SCHEMA = {'type': 'object', 'properties': {'value': {'type': 'string'}}, 'required': ['value'], 'additionalProperties': False}


def binding():
    return {'connection': {'connection': str(uuid.uuid4()), 'revision': 2, 'credential_epoch': 1},
            'catalog_revision': 1, 'grant_revision': 1, 'name': 'echo',
            'schema_digest': runtime.digest(SCHEMA)}


def permit(root, mode='normal'):
    return {'type': 'local', 'id': str(uuid.uuid4()), 'transport': {'type': 'worker_stdio', 'worker': str(uuid.uuid4()),
            'account': {'uid': os.getuid(), 'gid': os.getgid()}, 'executable': sys.executable,
            'arguments': [str(Path(__file__).with_name('m06_stdio_fixture.py').resolve()), mode],
            'cwd': str(root), 'environment': {'FIXTURE_EVIDENCE': str(root / 'effects.jsonl')}},
            'secret': {'type': 'local_environment', 'values': {'FIXTURE_TOKEN': 'synthetic-local-private-token'}},
            'deadline': int(time.time()) + 10, 'max_result_bytes': 8192,
            'operation': {'type': 'call', 'proposal': {'tool': binding(), 'arguments': {'value': 'hello'}}}}


class Contract(unittest.TestCase):
    def test_worker_and_manager_digest_vectors_match(self):
        vectors = json.loads((Path(__file__).parents[2] / 'link/schema/connector-digests.json').read_text())
        for vector in vectors:
            self.assertEqual(runtime.digest(vector['value']), vector['digest'])

    def test_real_stdio_has_clean_environment_redacts_results_and_bounds_failure(self):
        os.environ['UNRELATED_HOST_SECRET'] = 'must-not-inherit'
        for mode, expected in [('normal', 'finished'), ('wrong-version', 'not_sent'), ('lost', 'unknown'), ('oversized', 'unknown'), ('deadline', 'unknown')]:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                launch = permit(root, mode)
                if mode == 'deadline':
                    launch['deadline'] = time.time() + .5
                result = runtime.Stdio(launch).execute()
                self.assertEqual(result['type'], expected)
                self.assertNotIn('synthetic-local-private-token', json.dumps(result))
                effects = (root / 'effects.jsonl').read_text().splitlines() if (root / 'effects.jsonl').exists() else []
                self.assertEqual(len(effects), int(mode != 'wrong-version'))

    def test_credentials_matching_envelope_fields_preserve_the_saved_outcome(self):
        for secret in ('type', 'tool', 'value'):
            with self.subTest(secret=secret), tempfile.TemporaryDirectory() as directory:
                launch = permit(Path(directory))
                launch['secret']['values']['FIXTURE_TOKEN'] = secret
                outcome = runtime.Stdio(launch).execute()
                self.assertEqual(outcome['type'], 'finished')
                self.assertEqual(outcome['result']['type'], 'tool')
                self.assertEqual(outcome['result']['value']['content'][0]['text'], '[redacted]hello')

    def test_recheck_after_discovery_prevents_the_tool_effect(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = runtime.Stdio(permit(root), before_call=lambda: False).execute()
            self.assertEqual(result, {'type': 'not_sent', 'reason': 'authority'})
            self.assertFalse((root / 'effects.jsonl').exists())

    def test_live_revocation_interrupts_stdio_with_an_unknown_effect(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            launch = permit(root, 'deadline')
            result = runtime.Stdio(launch, lambda: not (root / 'effects.jsonl').exists()).execute()
            self.assertEqual(result, {'type': 'unknown', 'reason': 'authority'})

    def test_lost_launch_response_never_launches_on_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            launch = {'directory': str(root), 'task': str(uuid.uuid4()), 'execution': str(uuid.uuid4())}
            p = permit(root)
            events = []
            class Gateway(runtime.Gateway):
                def request(self, body):
                    events.append(body['type'])
                    if body['type'] == 'execute':
                        raise runtime.ConnectorError('transport', True)
                    return {'type': 'status', 'call': {'state': {'type': 'dispatch_possible'}}}
            gateway = Gateway(launch, {})
            with self.assertRaises(runtime.ConnectorError):
                gateway.execute(p['id'], p['operation'])
            with self.assertRaises(runtime.ConnectorError):
                Gateway(launch, {}).execute(p['id'], p['operation'])
            self.assertEqual(events, ['execute', 'status'])
            self.assertFalse((root / 'effects.jsonl').exists())

    def test_lost_settlement_response_resubmits_captured_evidence_only(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            launch = {'directory': str(root), 'task': str(uuid.uuid4()), 'execution': str(uuid.uuid4())}
            p = permit(root)
            events = []
            outcomes = []
            class Gateway(runtime.Gateway):
                def request(self, body):
                    events.append(body['type'])
                    if body['type'] == 'execute':
                        return p
                    if body['type'] == 'status':
                        return {'type': 'status', 'call': {'state': {'type': 'dispatch_possible'}}}
                    outcomes.append(body['outcome'])
                    if len(outcomes) == 1:
                        raise runtime.ConnectorError('transport', True)
                    return {'type': 'status', 'call': {'id': p['id'], 'state': {'type': 'resolved', 'outcome': body['outcome']}}}
            with self.assertRaises(runtime.ConnectorError):
                Gateway(launch, {}).execute(p['id'], p['operation'])
            call = Gateway(launch, {}).execute(p['id'], p['operation'])
            self.assertEqual(call['state']['outcome']['type'], 'finished')
            self.assertEqual(events.count('execute'), 1)
            self.assertEqual(outcomes[0], outcomes[1])
            self.assertEqual(len((root / 'effects.jsonl').read_text().splitlines()), 1)
            self.assertNotIn('synthetic-local-private-token', (root / 'connector-calls' / (p['id'] + '.json')).read_text())

    def test_pin_transport_completes_a_client_cert_call(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cert, key = root / 'cert.pem', root / 'key.pem'
            subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(key),
                            '-out', str(cert), '-days', '1', '-subj', '/CN=localhost',
                            '-addext', 'subjectAltName=IP:127.0.0.1',
                            '-addext', 'extendedKeyUsage=serverAuth,clientAuth'],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            key.chmod(0o600)
            seen = []

            class Manager(BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_POST(self):
                    seen.append({'path': self.path, 'body': json.loads(self.rfile.read(int(self.headers['Content-Length'])))})
                    data = b'{"type":"catalog","tools":[]}'
                    self.send_response(200)
                    self.send_header('Content-Type', 'application/json')
                    self.send_header('Content-Length', str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)

            manager = ThreadingHTTPServer(('127.0.0.1', 0), Manager)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(cert, key)
            context.load_verify_locations(cert)
            context.verify_mode = ssl.CERT_REQUIRED
            manager.socket = context.wrap_socket(manager.socket, server_side=True)
            threading.Thread(target=manager.serve_forever, daemon=True).start()
            try:
                launch = {'directory': str(root), 'task': str(uuid.uuid4()), 'execution': str(uuid.uuid4())}
                pin = {
                    'manager': f'wss://127.0.0.1:{manager.server_port}',
                    'pin': '0' * 64,
                    'client_cert': str(cert),
                    'client_key': str(key),
                }
                self.assertEqual(runtime.Gateway(launch, pin).request({'type': 'catalog'}),
                                 {'type': 'catalog', 'tools': []})
                self.assertEqual(seen[-1]['body']['type'], 'catalog')
                leftover = dict(pin)
                leftover.pop('pin')
                leftover['manager_' + 'ca'] = str(cert)
                with self.assertRaises((KeyError, runtime.ConnectorError)):
                    runtime.Gateway(launch, leftover).request({'type': 'catalog'})
            finally:
                manager.shutdown()
                manager.server_close()

    def test_rcgen_ed25519_worker_key_reaches_the_manager(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cert, key, wrapped = root / 'cert.pem', root / 'key.pem', root / 'rcgen.key'
            subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ed25519', '-nodes', '-keyout', str(key),
                            '-out', str(cert), '-days', '1', '-subj', '/CN=localhost',
                            '-addext', 'subjectAltName=IP:127.0.0.1',
                            '-addext', 'extendedKeyUsage=serverAuth,clientAuth'],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            key.chmod(0o600)
            pem = key.read_bytes()
            lines = [line for line in pem.splitlines() if line and not line.startswith(b'-----')]
            der = base64.b64decode(b''.join(lines))
            oid = bytes.fromhex('06032b6570')
            index = der.find(oid)
            seed = der[index + len(oid) + 4:index + len(oid) + 36]
            public = subprocess.check_output(['openssl', 'pkey', '-in', str(key), '-pubout', '-outform', 'DER'])[-32:]
            body = bytes.fromhex('020101300506032b657004220420') + seed + bytes([0xa1, 0x21, 0x00]) + public
            out = bytes([0x30, len(body)]) + body
            packed = base64.encodebytes(out).replace(b'\n', b'')
            chunks = [packed[i:i + 64] for i in range(0, len(packed), 64)]
            wrapped.write_bytes(b'-----BEGIN PRIVATE KEY-----\n' + b'\n'.join(chunks) + b'\n-----END PRIVATE KEY-----\n')
            wrapped.chmod(0o600)
            with self.assertRaises(ssl.SSLError):
                context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
                context.load_cert_chain(cert, wrapped)
            seen = []

            class Manager(BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_POST(self):
                    seen.append(self.path)
                    data = b'{"type":"catalog","tools":[]}'
                    self.send_response(200)
                    self.send_header('Content-Type', 'application/json')
                    self.send_header('Content-Length', str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)

            manager = ThreadingHTTPServer(('127.0.0.1', 0), Manager)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(cert, key)
            context.load_verify_locations(cert)
            context.verify_mode = ssl.CERT_REQUIRED
            manager.socket = context.wrap_socket(manager.socket, server_side=True)
            threading.Thread(target=manager.serve_forever, daemon=True).start()
            try:
                launch = {'directory': str(root), 'task': str(uuid.uuid4()), 'execution': str(uuid.uuid4())}
                pin = {
                    'manager': f'wss://127.0.0.1:{manager.server_port}',
                    'pin': '0' * 64,
                    'client_cert': str(cert),
                    'client_key': str(wrapped),
                }
                self.assertEqual(runtime.Gateway(launch, pin).request({'type': 'catalog'}),
                                 {'type': 'catalog', 'tools': []})
                self.assertTrue(seen[-1].startswith('/worker/v1/connectors/'))
            finally:
                manager.shutdown()
                manager.server_close()


if __name__ == '__main__':
    unittest.main()
