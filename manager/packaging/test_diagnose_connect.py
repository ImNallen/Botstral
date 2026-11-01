from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import ssl
import subprocess
import tempfile
import threading
import unittest

import diagnose


def ed25519_identity(root):
    cert, key = root / 'cert.pem', root / 'key.pem'
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ed25519', '-nodes', '-keyout', str(key),
                    '-out', str(cert), '-days', '1', '-subj', '/CN=localhost',
                    '-addext', 'subjectAltName=IP:127.0.0.1',
                    '-addext', 'extendedKeyUsage=serverAuth,clientAuth'],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    key.chmod(0o600)
    cert.chmod(0o600)
    pem = subprocess.check_output(['openssl', 'x509', '-in', str(cert), '-pubkey', '-noout'])
    der = subprocess.check_output(['openssl', 'pkey', '-pubin', '-outform', 'DER'], input=pem)
    return cert, key, der[-32:].hex()


class DiagnoseConnectTests(unittest.TestCase):
    def test_matching_pin_connects_and_wrong_pin_is_refused_before_any_request(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cert, key, pin = ed25519_identity(root)
            seen = []

            class Manager(BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_GET(self):
                    seen.append(self.path)
                    self.send_response(200)
                    self.end_headers()

                def do_POST(self):
                    seen.append(self.path)
                    self.send_response(200)
                    self.end_headers()

            manager = ThreadingHTTPServer(('127.0.0.1', 0), Manager)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(cert, key)
            context.load_verify_locations(cert)
            context.verify_mode = ssl.CERT_REQUIRED
            manager.socket = context.wrap_socket(manager.socket, server_side=True)
            threading.Thread(target=manager.serve_forever, daemon=True).start()
            try:
                config = {
                    'manager': f'wss://127.0.0.1:{manager.server_port}',
                    'pin': pin,
                    'client_cert': str(cert),
                    'client_key': str(key),
                }
                diagnose.connectivity(config)
                with self.assertRaises(ValueError):
                    diagnose.connectivity(dict(config, pin='0' * 64))
                self.assertEqual(seen, [])
            finally:
                manager.shutdown()
                manager.server_close()


if __name__ == '__main__':
    unittest.main()
