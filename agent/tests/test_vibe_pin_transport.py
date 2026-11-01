import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
from pathlib import Path
import shutil
import ssl
import subprocess
import tempfile
import threading
import unittest
import uuid

AGENT = Path(__file__).resolve().parents[1]
module = importlib.util.spec_from_file_location('managed_vibe', AGENT / 'vibe/runtime.py')
runtime = importlib.util.module_from_spec(module)
module.loader.exec_module(runtime)


def ed25519_identity(root):
    cert, key = root / 'cert.pem', root / 'key.pem'
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ed25519', '-nodes', '-keyout', str(key),
                    '-out', str(cert), '-days', '1', '-subj', '/CN=localhost',
                    '-addext', 'subjectAltName=IP:127.0.0.1',
                    '-addext', 'extendedKeyUsage=serverAuth,clientAuth'],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    key.chmod(0o600)
    pem = subprocess.check_output(['openssl', 'x509', '-in', str(cert), '-pubkey', '-noout'])
    der = subprocess.check_output(['openssl', 'pkey', '-pubin', '-outform', 'DER'], input=pem)
    return cert, key, der[-32:].hex()


class VibePinTransportTests(unittest.TestCase):
    def test_relay_accepts_the_matching_pin_and_refuses_a_wrong_pin_before_any_request(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cert, key, pin = ed25519_identity(root)
            seen = []

            class Manager(BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_POST(self):
                    seen.append({'path': self.path, 'body': self.rfile.read(int(self.headers['Content-Length']))})
                    data = b'{"id":"fixture"}'
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
            shutil.copyfile(AGENT / 'connector/runtime.py', root / 'connector-runtime.py')
            launch = {'directory': str(root), 'task': str(uuid.uuid4()), 'execution': str(uuid.uuid4())}
            transport = {
                'manager': f'wss://127.0.0.1:{manager.server_port}',
                'pin': pin,
                'client_cert': str(cert),
                'client_key': str(key),
            }
            relay = runtime.Relay(launch, transport)
            threading.Thread(target=relay.serve_forever, daemon=True).start()
            try:
                body = b'{"model":"fixture","messages":[]}'
                headers = {
                    'Authorization': 'Bearer ' + relay.token,
                    'Content-Type': 'application/json',
                    'Content-Length': str(len(body)),
                }
                connection = http.client.HTTPConnection('127.0.0.1', relay.server_port, timeout=5)
                connection.request('POST', '/v1/chat/completions', body, headers)
                response = connection.getresponse()
                data = response.read()
                connection.close()
                self.assertEqual(response.status, 200)
                self.assertEqual(data, b'{"id":"fixture"}')
                self.assertTrue(seen[-1]['path'].startswith('/worker/v1/coding/'))
                accepted = len(seen)
                for wrong in ('0' * 64, '1' * 64):
                    refused = runtime.Relay(launch, dict(transport, pin=wrong))
                    threading.Thread(target=refused.serve_forever, daemon=True).start()
                    try:
                        bad = http.client.HTTPConnection('127.0.0.1', refused.server_port, timeout=5)
                        bad.request('POST', '/v1/chat/completions', body, {
                            'Authorization': 'Bearer ' + refused.token,
                            'Content-Type': 'application/json',
                            'Content-Length': str(len(body)),
                        })
                        reply = bad.getresponse()
                        reply.read()
                        bad.close()
                        self.assertNotEqual(reply.status, 200)
                    finally:
                        refused.shutdown()
                        refused.server_close()
                self.assertEqual(len(seen), accepted)
            finally:
                relay.shutdown()
                relay.server_close()
                manager.shutdown()
                manager.server_close()


if __name__ == '__main__':
    unittest.main()
