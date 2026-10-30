"""A recorded HTTPS provider confined to a disposable acceptance host."""
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import ssl
import subprocess
import threading


@contextmanager
def provider(root):
    certificate = root / 'provider.pem'
    key = root / 'provider.key'
    ca = root / 'provider-ca.pem'
    ca_key = root / 'provider-ca.key'
    def openssl(*arguments):
        subprocess.run(['openssl', *map(str, arguments)], check=True, capture_output=True)
    openssl('req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=Botstral release fixture CA', '-keyout', ca_key, '-out', ca)
    openssl('req', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=api.mistral.ai', '-keyout', key, '-out', root / 'provider.csr')
    extensions = root / 'provider.ext'
    extensions.write_text('subjectAltName=DNS:api.mistral.ai\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n')
    openssl('x509', '-req', '-in', root / 'provider.csr', '-CA', ca, '-CAkey', ca_key, '-CAcreateserial', '-days', '1', '-extfile', extensions, '-out', certificate)
    ca.chmod(0o644)
    observed = []
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            authorized = self.headers.get('Authorization') == 'Bearer synthetic restored provider credential'
            observed.append(authorized)
            body = json.dumps({'id': 'mistral-small-2506', 'max_context_length': 32768,
                               'capabilities': {'completion_chat': True, 'function_calling': False}}).encode()
            self.send_response(200 if authorized else 401)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        def log_message(self, *args):
            pass
    server = HTTPServer(('127.0.0.1', 443), Handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certificate, key)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    hosts = Path('/etc/hosts')
    original_hosts = hosts.read_bytes()
    original_ca = os.environ.get('SSL_CERT_FILE')
    try:
        with hosts.open('ab') as stream:
            stream.write(b'\n127.0.0.1 api.mistral.ai\n')
        os.environ['SSL_CERT_FILE'] = str(ca)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        yield observed
    finally:
        if 'thread' in locals():
            server.shutdown()
        server.server_close()
        hosts.write_bytes(original_hosts)
        if original_ca is None:
            os.environ.pop('SSL_CERT_FILE', None)
        else:
            os.environ['SSL_CERT_FILE'] = original_ca
