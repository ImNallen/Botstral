#!/usr/bin/env python3
"""Exercise production Manager and native owner binaries with synthetic private state."""
import argparse
import base64
import hashlib
from http.client import HTTPConnection, HTTPSConnection
import json
import os
from pathlib import Path
import shutil
import signal
import select
import socket
import sqlite3
import ssl
import struct
import subprocess
import tempfile
import threading
import time
import uuid


def require(value, message):
    if not value:
        raise AssertionError(message)


def write(path, value):
    path.write_text(json.dumps(value))
    path.chmod(0o600)


def command(args, **kw):
    return subprocess.run(list(map(str, args)), check=True, capture_output=True, **kw)


def http(address, method, path, body=None, token=None, context=None, headers=None):
    host, port = address.rsplit(':', 1)
    cls = HTTPSConnection if context else HTTPConnection
    options = {'context': context} if context else {}
    connection = cls(host, int(port), timeout=10, **options)
    head = {'Content-Type': 'application/json', 'Connection': 'close'}
    if token:
        head['Authorization'] = 'Bearer ' + token
    head.update(headers or {})
    try:
        connection.request(method, path, json.dumps(body or {}), head)
        response = connection.getresponse()
        raw = response.read()
        return response.status, json.loads(raw) if raw else None
    finally:
        connection.close()


class WebSocket:
    def __init__(self, address, context, hostname='localhost', path='/owner/v1/connect'):
        self.notifications=[]
        host, port = address.rsplit(':', 1)
        self.socket = context.wrap_socket(socket.create_connection((host, int(port)), timeout=5), server_hostname=hostname)
        self.socket.settimeout(8)
        key = base64.b64encode(os.urandom(16)).decode()
        self.socket.sendall((f'GET {path} HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n').encode())
        response = b''
        while not response.endswith(b'\r\n\r\n'):
            response += self.exact(1)
        require(response.startswith(b'HTTP/1.1 101'), 'WebSocket upgrade refused')

    def exact(self, count):
        data = b''
        while len(data) < count:
            chunk = self.socket.recv(count - len(data))
            if not chunk:
                raise EOFError('peer closed')
            data += chunk
        return data

    def send(self, value):
        data = json.dumps(value).encode()
        mask = os.urandom(4)
        size = len(data)
        length = bytes([0x80 | size]) if size < 126 else b'\xfe' + struct.pack('!H', size)
        self.socket.sendall(b'\x81' + length + mask + bytes(v ^ mask[i % 4] for i, v in enumerate(data)))

    def receive(self):
        first, second = self.exact(2)
        if first & 15 == 8:
            raise EOFError('WebSocket closed')
        size = second & 127
        if size == 126:
            size = struct.unpack('!H', self.exact(2))[0]
        elif size == 127:
            size = struct.unpack('!Q', self.exact(8))[0]
        require(size < 65536, 'frame exceeds existing message limit')
        value=json.loads(self.exact(size))
        if value.get('type')=='notification':
            self.notifications.append(value['head'])
        return value

    def close(self):
        self.socket.close()

    def call(self, operation):
        request = str(uuid.uuid4())
        self.send({'type':'call', 'id':request, 'operation':operation})
        start = self.receive()
        while start['type'] in ('pong', 'notification'):
            start = self.receive()
        require(start['type'] == 'transfer_start' and start['request'] == request, 'operation did not start a transfer')
        content = bytearray()
        for index in range(start['chunks']):
            chunk = self.receive()
            while chunk['type'] in ('pong', 'notification'):
                chunk = self.receive()
            require(chunk['type'] == 'transfer_chunk' and chunk['id'] == start['id'] and chunk['index'] == index, 'transfer order or identity')
            content.extend(base64.b64decode(chunk['data'], validate=True))
        complete = self.receive()
        while complete['type'] in ('pong', 'notification'):
            complete = self.receive()
        require(complete == {'type':'transfer_complete', 'id':start['id']}, 'missing transfer completion')
        require(len(content) == start['bytes'] and hashlib.sha256(content).hexdigest() == start['digest'], 'transfer integrity')
        return json.loads(content)


class OpaqueProxy:
    def __init__(self, upstream, delay=.003):
        self.upstream = upstream
        self.delay=delay
        self.listener = socket.socket()
        self.listener.bind(('127.0.0.1', 0))
        self.listener.listen()
        self.listener.settimeout(.1)
        self.address = '127.0.0.1:'+str(self.listener.getsockname()[1])
        self.stop = threading.Event()
        self.discard_server=threading.Event()
        self.accepted=0
        self.first_end=None
        self.server_bytes=0
        self.client_bytes=0
        self.sockets = []
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        while not self.stop.is_set():
            try:
                client, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            host, port = self.upstream.rsplit(':', 1)
            self.accepted+=1
            connection_number=self.accepted
            server = socket.create_connection((host, int(port)), timeout=5)
            self.sockets += [client, server]
            try:
                while not self.stop.is_set():
                    ready, _, _ = select.select([client, server], [], [], .1)
                    for source in ready:
                        data = source.recv(4096)
                        if not data:
                            if connection_number==1:
                                self.first_end='client' if source is client else 'server'
                            raise EOFError()
                        if source is client:
                            self.client_bytes+=len(data)
                        else:
                            self.server_bytes+=len(data)
                        if source is client or not self.discard_server.is_set():
                            (server if source is client else client).sendall(data)
                        if source is server and self.delay:
                            time.sleep(self.delay)
            except (OSError, EOFError):
                pass
            finally:
                client.close()
                server.close()

    def close(self):
        self.stop.set()
        self.listener.close()
        for stream in self.sockets:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            stream.close()
        self.thread.join(timeout=3)
        require(not self.thread.is_alive(), 'fixture proxy did not stop')


def native_boundary(f, directory, wrong_leaf):
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen()
    listener.settimeout(8)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    cert = 'replacement' if wrong_leaf else 'manager'
    context.load_cert_chain(f.root/(cert+'.pem'), f.root/(cert+'.key'))
    observed = {'application_bytes':0, 'connections':0, 'mutations':0}
    config = json.loads((directory/'device.json').read_text())

    def peer():
        try:
            raw, _ = listener.accept()
            raw.settimeout(5)
            observed['connections'] += 1
            with context.wrap_socket(raw, server_side=True) as connection:
                head = b''
                while not head.endswith(b'\r\n\r\n'):
                    part=connection.recv(1)
                    if not part:
                        raise EOFError()
                    head += part
                observed['application_bytes'] += len(head)
                if wrong_leaf:
                    return
                key = next(line.split(b':',1)[1].strip() for line in head.split(b'\r\n') if line.lower().startswith(b'sec-websocket-key:'))
                accept = base64.b64encode(hashlib.sha1(key+b'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest())
                connection.sendall(b'HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: '+accept+b'\r\n\r\n')
                def exact(count):
                    data = b''
                    while len(data) < count:
                        part = connection.recv(count-len(data))
                        if not part:
                            raise EOFError()
                        data += part
                    return data
                def receive():
                    _, count = exact(2)
                    count &= 127
                    if count == 126:
                        count = struct.unpack('!H', exact(2))[0]
                    mask = exact(4)
                    body = exact(count)
                    return json.loads(bytes(v ^ mask[i%4] for i,v in enumerate(body)))
                hello = receive()
                require(hello['type'] == 'authenticate', 'stub expected saved authentication')
                welcome = json.dumps({'type':'welcome','version':1,'manager':config['trust']['manager'],'device':config['device']}).encode()
                connection.sendall(b'\x81\x7e'+struct.pack('!H',len(welcome))+welcome)
                require(receive()['type'] == 'watch', 'native client did not subscribe before recovery')
                call = receive()
                require(call['type'] == 'call', 'stub expected one owner operation')
                observed['mutations'] += 1
        except (ssl.SSLError, EOFError, OSError):
            pass
        finally:
            listener.close()

    thread = threading.Thread(target=peer, daemon=True)
    thread.start()
    profile = dict(f.profile(), endpoint='127.0.0.1:'+str(listener.getsockname()[1]))
    write(f.root/'boundary-trust.json', profile)
    command([f.probe,'trust',directory,f.root/'boundary-trust.json'])
    operation = {'method':'resume','cursor':0,'limit':64} if wrong_leaf else {'method':'append_message','input':{'id':str(uuid.uuid4()),'conversation':str(uuid.uuid4()),'text':'unknown outcome fixture'}}
    failure = f.call(directory, operation, success=False)
    thread.join(timeout=8)
    require(not thread.is_alive(), 'native boundary fixture did not terminate')
    if wrong_leaf:
        require(b'Trust' in failure.stderr and observed['application_bytes'] == 0, 'native client transmitted application bytes before exact pin verification')
    else:
        require(b'UnknownOutcome' in failure.stderr and observed['connections'] == 1 and observed['mutations'] == 1, 'native client repeated or misclassified an unknown mutation')
    write(f.root/'boundary-direct.json', f.profile())
    command([f.probe,'trust',directory,f.root/'boundary-direct.json'])


def certificates(root):
    def openssl(*args):
        command(['openssl', *args])
    openssl('req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '2', '-subj', '/CN=Synthetic M05 CA', '-keyout', root/'ca.key', '-out', root/'ca.pem')
    for name, usage in [('manager', 'serverAuth'), ('replacement', 'serverAuth'), ('worker', 'clientAuth')]:
        openssl('req', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=Synthetic M05 '+name, '-keyout', root/(name+'.key'), '-out', root/(name+'.csr'))
        (root/(name+'.ext')).write_text('subjectAltName=DNS:localhost,DNS:bore.pub,IP:127.0.0.1,IP:192.168.1.40\nextendedKeyUsage='+usage+'\n')
        openssl('x509', '-req', '-in', root/(name+'.csr'), '-CA', root/'ca.pem', '-CAkey', root/'ca.key', '-CAcreateserial', '-days', '2', '-extfile', root/(name+'.ext'), '-out', root/(name+'.pem'))
    for path in root.iterdir():
        path.chmod(0o600)


class Fixture:
    def __init__(self, root, manager, probe, agent):
        self.root, self.manager, self.probe, self.agent = root, manager, probe, agent
        self.worker = None
        self.process = None
        self.token = None
        self.addresses = None
        self.generation = 0
        self.context = ssl.create_default_context(cafile=str(root/'ca.pem'))
        self.context.check_hostname = False

    def start(self, cert='manager'):
        self.certificate = cert
        state = self.root/'state'
        (state/'listeners.json').unlink(missing_ok=True)
        address = self.addresses['public'] if self.addresses else '127.0.0.1:0'
        local = self.addresses['listen'] if self.addresses else '127.0.0.1:0'
        self.generation += 1
        self.log = self.root/f'manager-{self.generation}.log'
        with self.log.open('wb') as log:
            self.process = subprocess.Popen([self.manager, '--state-dir', state, '--listen', local, '--setup-listen', '127.0.0.1:0', '--public-listen', address, '--public-tls-cert', self.root/(cert+'.pem'), '--public-tls-key', self.root/(cert+'.key'), '--public-worker-ca', self.root/'ca.pem'], stdout=log, stderr=log)
        deadline = time.monotonic()+15
        while time.monotonic() < deadline:
            require(self.process.poll() is None, 'Manager startup failed')
            if (state/'listeners.json').exists():
                self.addresses = json.loads((state/'listeners.json').read_text())
                return
            time.sleep(.02)
        raise TimeoutError('Manager readiness')

    def stop(self):
        if self.process and self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            require(self.process.wait(timeout=10) == 0, 'Manager shutdown failed')

    def owner(self, method, path, body=None, status=200):
        code, value = http(self.addresses['listen'], method, path, body, self.token)
        require(code == status, f'local owner response {code} for {path}')
        return value

    def setup(self):
        proof = (self.root/'state/bootstrap.token').read_text()
        code, _ = http(self.addresses['setup'], 'POST', '/v1/owner', {'name':'Synthetic M05 owner', 'password':'synthetic M05 password', 'vault_passphrase':'synthetic M05 vault passphrase', 'acknowledge_no_escrow':True}, proof)
        require(code == 201, 'local setup failed')
        self.login()

    def login(self):
        self.token = self.owner('POST', '/v1/sessions', {'password':'synthetic M05 password'}, status=201)['token']

    def profile(self, cert=None):
        cert = cert or self.certificate
        return self.owner('POST', '/v1/device-trust', {'endpoint':self.addresses['public'], 'server_name':'localhost', 'ca_pem':(self.root/'ca.pem').read_text(), 'certificate_pem':(self.root/(cert+'.pem')).read_text()})

    def client(self, name):
        directory = self.root/name
        request = json.loads(command([self.probe, 'init', directory, name]).stdout)
        grant = self.owner('POST', '/v1/device-invitations', {'request':request, 'lifetime_seconds':600})
        bundle = {'trust':self.profile(), **{key:grant[key] for key in ('role','fingerprint','expires_at','invitation')}}
        path = self.root/(name+'-invitation.json')
        write(path, bundle)
        command([self.probe, 'import', directory, path])
        return directory

    def call(self, directory, operation, success=True):
        path = self.root/'operation.json'
        write(path, operation)
        result = subprocess.run([self.probe, 'call', directory, path], capture_output=True, timeout=130)
        require((result.returncode == 0) == success, 'native call exit status')
        return json.loads(result.stdout) if success else result

    def socket(self, directory):
        config = json.loads((directory/'device.json').read_text())
        ws = WebSocket(self.addresses['public'], self.context)
        ws.send({'type':'authenticate', 'version':1, 'manager':config['trust']['manager'], 'role':'owner', 'credential':config['credential']})
        require(ws.receive()['type'] == 'welcome', 'device authentication failed')
        return ws

    def start_worker(self):
        fingerprint=hashlib.sha256(ssl.PEM_cert_to_DER_cert((self.root/'worker.pem').read_text())).hexdigest()
        grant=self.owner('POST','/v1/worker-invitations',{'fingerprint':fingerprint,'lifetime_seconds':600},status=201)
        write(self.root/'worker-invitation.json',grant)
        state=self.root/'worker-state'
        command([self.agent,'enroll','--state-dir',state,'--manager','wss://'+self.addresses['public'],'--manager-ca',self.root/'ca.pem','--client-cert',self.root/'worker.pem','--client-key',self.root/'worker.key','--invitation-file',self.root/'worker-invitation.json'],timeout=15)
        with (self.root/'worker.log').open('wb') as log:
            self.worker=subprocess.Popen([self.agent,'run','--state-dir',state],stdout=log,stderr=log)
        deadline=time.monotonic()+10
        while time.monotonic()<deadline:
            require(self.worker.poll() is None,'production worker exited')
            workers=self.owner('GET','/v1/workers')
            if workers and workers[0]['phase']=='ready':
                return workers[0]['id']
            time.sleep(.05)
        raise TimeoutError('production worker readiness')

    def stop_worker(self):
        if self.worker and self.worker.poll() is None:
            self.worker.send_signal(signal.SIGTERM)
            require(self.worker.wait(timeout=10)==0,'worker shutdown failed')


def exercise(f, checks):
    def passed(name):
        checks.append(name)
        print('PASS '+name, flush=True)
    f.start()
    for path in ['/v1/owner', '/v1/sessions', '/v1/device-invitations']:
        code, _ = http(f.addresses['public'], 'POST', path, {}, context=f.context, headers={'Forwarded':'for=127.0.0.1', 'X-Forwarded-For':'127.0.0.1'})
        require(code == 404, 'public setup or local administration exposed')
    passed('public_setup_absent_before_owner_with_forwarded_headers')
    f.setup()
    code, _ = http(f.addresses['public'], 'POST', '/worker/v1/enroll', {}, context=f.context)
    require(code == 403, 'worker route accepted anonymous TLS')
    worker = ssl.create_default_context(cafile=str(f.root/'ca.pem'))
    worker.load_cert_chain(f.root/'worker.pem', f.root/'worker.key')
    code, _ = http(f.addresses['public'], 'GET', '/owner/v1/connect', context=worker)
    require(code == 403, 'worker certificate reached owner protocol')
    passed('separate_worker_and_owner_admission')
    directory = f.client('native')
    reply = f.call(directory, {'method':'resume', 'cursor':0, 'limit':64})
    require(reply['type'] == 'replay', 'native pairing and snapshot')
    command([f.probe, 'watch', directory, '1'], timeout=8)
    initial_cursor = json.loads((directory/'state.json').read_text())['cursor']
    passed('native_persisted_pairing_and_authenticated_snapshot')
    missing=f.call(directory,{'method':'task','id':str(uuid.uuid4())},success=False)
    require(b'Refused(NotFound)' in missing.stderr,'remote method changed the domain error')
    native_boundary(f, directory, True)
    native_boundary(f, directory, False)
    passed('exact_pin_precedes_all_application_bytes_and_unknown_mutations_are_not_replayed')
    raced = f.client('raced-enrollment')
    racing = json.loads((raced/'device.json').read_text())
    barrier = threading.Barrier(2)
    outcomes = []
    def redeem():
        ws = WebSocket(f.addresses['public'], f.context)
        try:
            barrier.wait(timeout=5)
            ws.send({'type':'enroll','version':1,'manager':racing['trust']['manager'],'role':'owner','credential':racing['credential'],'invitation':racing['invitation']})
            outcomes.append(ws.receive()['type'])
        finally:
            ws.close()
    threads = [threading.Thread(target=redeem) for _ in range(2)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(timeout=8)
    require(sorted(outcomes) == ['error','welcome'], 'concurrent redemption did not consume exactly once')
    f.call(raced, {'method':'resume','cursor':0,'limit':64})
    passed('concurrent_native_credential_enrollment_consumes_invitation_once')
    worker_id=f.start_worker()
    occupied=[f.socket(directory) for _ in range(8)]
    busy=f.client('capacity-retry')
    with (f.root/'capacity-retry.log').open('wb') as log:
        retry=subprocess.Popen([f.probe,'watch',busy,'4'],stdout=log,stderr=log)
    try:
        time.sleep(1)
        require(retry.poll() is None,'native client treated capacity refusal as terminal authentication')
        occupied.pop().close()
        require(retry.wait(timeout=8)==0,'native client failed to reconnect after capacity became available')
        require((busy/'state.json').exists(),'native client did not recover after capacity refusal')
    finally:
        for connection in occupied:
            connection.close()
        if retry.poll() is None:
            retry.kill()
            retry.wait()
    passed('bounded_device_capacity_retries_without_discarding_credentials_or_trust')
    bot = str(uuid.uuid4())
    f.owner('PUT', '/v1/bots/'+bot, {'name':'Snapshot fixture'})
    conversation = str(uuid.uuid4())
    subscribed=f.socket(directory)
    subscribed.send({'type':'watch','cursor':0})
    f.owner('POST', '/v1/conversations', {'id':conversation, 'bot':bot})
    replay=subscribed.call({'method':'resume','cursor':0,'limit':64})['value']
    require(any(c['id']==conversation for c in replay['state']['conversations']),'commit between subscription and initial replay was lost')
    while not subscribed.notifications:
        subscribed.receive()
    require(max(subscribed.notifications)>=replay['cursor'],'committed event wake was lost around initial replay')
    subscribed.close()
    passed('subscribe_before_initial_replay_preserves_the_concurrent_commit_and_wake')
    messages = f.socket(directory)
    try:
        for _ in range(256):
            reply = messages.call({'method':'append_message', 'input':{'id':str(uuid.uuid4()), 'conversation':conversation, 'text':'\x01'*4096}})
            require(reply['type'] == 'message', 'owner message was not committed')
    finally:
        messages.close()
    for n in range(24):
        doc_bot = str(uuid.uuid4())
        f.owner('PUT', '/v1/bots/'+doc_bot, {'name':'Document '+str(n)})
        f.owner('PUT', '/v1/documents', {'bot':doc_bot, 'kind':'memory', 'expected_revision':0, 'sources':[{'type':'external','reference':'https://fixture.invalid/source'}], 'markdown':'m'*4096})
    reply = f.call(directory, {'method':'resume', 'cursor':1, 'limit':64})
    require(reply['value']['type'] == 'snapshot' and len(reply['value']['state']['messages']) == 256, 'retention rollover silently lost messages')
    require(len(json.dumps(reply)) > 65536, 'snapshot fixture is too small')
    docs = f.call(directory, {'method':'rebuild_documents'})
    require(len(docs['value']) == 24 and len(json.dumps(docs)) > 65536, 'large ordinary reply incomplete')
    passed('chunked_snapshot_and_ordinary_reply_above_64k_and_retention_rollover')
    proxy = OpaqueProxy(f.addresses['public'])
    changed = dict(f.profile(), endpoint=proxy.address)
    write(f.root/'proxy-trust.json', changed)
    command([f.probe, 'trust', directory, f.root/'proxy-trust.json'])
    with (f.root/'interrupted.log').open('wb') as log:
        interrupted = subprocess.Popen([f.probe, 'watch', directory, '5'], stdout=log, stderr=log)
    try:
        deadline = time.monotonic()+4
        while time.monotonic() < deadline:
            partial = directory/'.transfer.part'
            if partial.exists() and partial.stat().st_size > 32768:
                break
            time.sleep(.01)
        require(partial.exists() and partial.stat().st_size > 32768, 'native recovery did not enter staging')
        proxy.close()
        require(interrupted.wait(timeout=8) == 0, 'transient interrupted recovery did not retry')
        require(json.loads((directory/'state.json').read_text())['cursor'] == initial_cursor, 'interrupted snapshot advanced application cursor')
        require(not partial.exists(), 'interrupted transfer staging was not cleaned')
    finally:
        proxy.close()
        if interrupted.poll() is None:
            interrupted.kill()
            interrupted.wait()
    write(f.root/'direct-trust.json', f.profile())
    command([f.probe, 'trust', directory, f.root/'direct-trust.json'])
    command([f.probe, 'watch', directory, '2'], timeout=8)
    require(len(json.loads((directory/'state.json').read_text())['state']['messages']) == 256, 'interrupted recovery omitted authoritative state')
    passed('interrupted_snapshot_preserves_cursor_and_recovers_after_retention_rollover')
    silent_bot=str(uuid.uuid4())
    f.owner('PUT','/v1/bots/'+silent_bot,{'name':'Dead-peer fixture'})
    previous=json.loads((directory/'state.json').read_text())['cursor']
    f.owner('PUT','/v1/bots/'+silent_bot+'/permission',{'expected_revision':1,'allow':False})
    silent=OpaqueProxy(f.addresses['public'],delay=0)
    write(f.root/'silent-trust.json',dict(f.profile(),endpoint=silent.address))
    command([f.probe,'trust',directory,f.root/'silent-trust.json'])
    with (f.root/'dead-peer.log').open('wb') as log:
        dead_peer=subprocess.Popen([f.probe,'watch',directory,'45'],stdout=log,stderr=log)
    try:
        deadline=time.monotonic()+12
        while time.monotonic()<deadline:
            if json.loads((directory/'state.json').read_text())['cursor']>previous:
                break
            time.sleep(.02)
        require(json.loads((directory/'state.json').read_text())['cursor']>previous,f'dead-peer fixture did not first recover normally, connections={silent.accepted}, first_end={silent.first_end}, server_bytes={silent.server_bytes}, client_bytes={silent.client_bytes}')
        silent.discard_server.set()
        require(dead_peer.wait(timeout=50)==0,'silent peer did not allow bounded reconnect')
        require(silent.accepted>=2 and silent.first_end=='client','client failed to detect a silent peer while TCP remained open')
    finally:
        silent.close()
        if dead_peer.poll() is None:
            dead_peer.kill()
            dead_peer.wait()
    command([f.probe,'trust',directory,f.root/'direct-trust.json'])
    passed('native_dead_peer_detection_reconnects_without_tcp_eof_or_pin_reset')
    slow = f.socket(directory)
    slow.socket.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 1024)
    slow.send({'type':'call','id':str(uuid.uuid4()),'operation':{'method':'resume','cursor':0,'limit':64}})
    require(slow.receive()['type'] == 'transfer_start', 'slow snapshot did not begin')
    transfer_started=time.monotonic()
    other = f.call(directory, {'method':'search_documents','bot':bot,'query':'missing'})
    require(other['type'] == 'documents', 'slow viewer blocked independent viewer')
    session=f.owner('POST','/v1/computer-sessions',{'bot_id':bot,'worker_id':worker_id})
    deadline=time.monotonic()+2
    while time.monotonic()<deadline:
        current=f.owner('GET','/v1/computer-sessions/'+session['id'])
        if current['status']=='confirmed':
            break
        time.sleep(.02)
    require(current['status']=='confirmed','slow viewer blocked real worker control confirmation')
    require(time.monotonic()-transfer_started<4,'slow-viewer check exceeded the blocked-write deadline')
    device = json.loads((directory/'device.json').read_text())['device']
    f.owner('DELETE', '/v1/devices/'+device, status=204)
    slow.socket.settimeout(3)
    closed = False
    started = time.monotonic()
    try:
        while True:
            slow.socket.recv(65536) or (_ for _ in ()).throw(EOFError())
    except (EOFError, ConnectionError, ssl.SSLError):
        closed = True
    finally:
        slow.close()
    require(closed and time.monotonic()-started < 3, 'revocation failed to interrupt blocked writer')
    failure = f.call(directory, {'method':'resume','cursor':0,'limit':64}, success=False)
    require(b'Authentication(Unauthorized)' in failure.stderr, 'revocation did not reject future native access')
    passed('slow_viewer_isolation_and_revocation_during_blocked_transfer')
    f.stop()
    f.start()
    f.login()
    f.call(directory, {'method':'resume','cursor':0,'limit':64}, success=False)
    passed('revoked_credential_rejected_after_manager_restart')
    prior_devices = len(f.owner('GET', '/v1/devices'))
    recovered = f.client('lost-response')
    config = json.loads((recovered/'device.json').read_text())
    lost = WebSocket(f.addresses['public'], f.context)
    lost.send({'type':'enroll', 'version':1, 'manager':config['trust']['manager'], 'role':'owner', 'credential':config['credential'], 'invitation':config['invitation']})
    lost.close()
    deadline = time.monotonic()+5
    while time.monotonic() < deadline:
        if len(f.owner('GET', '/v1/devices')) == prior_devices+1:
            break
        time.sleep(.02)
    require(len(f.owner('GET', '/v1/devices')) == prior_devices+1, 'lost-response enrollment did not commit')
    f.call(recovered, {'method':'resume', 'cursor':0, 'limit':64})
    require(len(f.owner('GET', '/v1/devices')) == prior_devices+1, 'lost response created another device')
    passed('lost_enrollment_response_recovers_with_persisted_credential')
    f.stop()
    f.start('replacement')
    f.login()
    failure = f.call(recovered, {'method':'resume','cursor':0,'limit':64}, success=False)
    require(b'Trust' in failure.stderr, 'same-CA replacement bypassed leaf pin')
    trust = f.profile()
    wrong = dict(trust, manager=str(uuid.uuid4()))
    write(f.root/'wrong-trust.json', wrong)
    refusal = subprocess.run([f.probe, 'trust', recovered, f.root/'wrong-trust.json'], capture_output=True)
    require(refusal.returncode != 0, 'replacement trust accepted another Manager identity')
    write(f.root/'replacement-trust.json', trust)
    command([f.probe, 'trust', recovered, f.root/'replacement-trust.json'])
    f.call(recovered, {'method':'resume','cursor':0,'limit':64})
    passed('same_ca_wrong_leaf_refusal_and_explicit_local_trust_recovery')
    watch_log = f.root/'native-watch.log'
    with watch_log.open('wb') as log:
        watch = subprocess.Popen([f.probe, 'watch', recovered, '80'], stdout=log, stderr=log)
    try:
        deadline = time.monotonic()+8
        while time.monotonic() < deadline and not (recovered/'state.json').exists():
            require(watch.poll() is None, 'native recovery exited before committing state')
            time.sleep(.02)
        require((recovered/'state.json').exists(), 'native client did not durably apply snapshot')
        before = json.loads((recovered/'state.json').read_text())['cursor']
        time.sleep(10)
        f.stop()
        f.start('replacement')
        f.login()
        f.owner('PUT', '/v1/bots/'+bot+'/permission', {'expected_revision':1,'allow':False})
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            if json.loads((recovered/'state.json').read_text())['cursor']>before:
                break
            time.sleep(.02)
        require(json.loads((recovered/'state.json').read_text())['cursor']>before,'native client did not recover after restart')
        def attempts():
            with sqlite3.connect(f.root/'state/manager.sqlite3') as database:
                return database.execute('SELECT window_start,attempts FROM remote_attempts').fetchone()
        authenticated=attempts()
        idle_started=time.monotonic()
        require(watch.wait(timeout=80) == 0, 'native reconnect or heartbeat failed')
        idle_elapsed=time.monotonic()-idle_started
        final_attempts=attempts()
        print(f'Heartbeat idle seconds={idle_elapsed:.3f}, auth_before={authenticated}, auth_after={final_attempts}',flush=True)
        require(idle_elapsed>30,f'heartbeat observation window was only {idle_elapsed:.3f} seconds')
        require(final_attempts==authenticated,f'idle native client reauthenticated, before={authenticated}, after={final_attempts}')
        after = json.loads((recovered/'state.json').read_text())
        require(after['cursor'] > before and any(not p['allow'] for p in after['state']['permissions']), 'native reconnect silently skipped committed events')
        require('notification head=' in watch_log.read_text(), 'native notification hint was not delivered')
        require(not (recovered/'.transfer.part').exists(), 'completed recovery left staging data')
    finally:
        if watch.poll() is None:
            watch.kill()
            watch.wait()
    passed('native_heartbeat_reconnect_notifications_and_atomic_application_cursor')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--manager-bin', type=Path, required=True)
    parser.add_argument('--probe-bin', type=Path, required=True)
    parser.add_argument('--agent-bin',type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix='m05-fixture-', dir=args.output))
    checks, fixture = [], None
    summary = {'checks':checks, 'manager_sha256':hashlib.sha256(args.manager_bin.read_bytes()).hexdigest(), 'probe_sha256':hashlib.sha256(args.probe_bin.read_bytes()).hexdigest()}
    try:
        certificates(root)
        agent=args.agent_bin or args.manager_bin.parent/'botstral-agent'
        summary['agent_sha256']=hashlib.sha256(agent.read_bytes()).hexdigest()
        fixture = Fixture(root, args.manager_bin.resolve(), args.probe_bin.resolve(),agent.resolve())
        exercise(fixture, checks)
        summary['passed'] = True
    finally:
        if fixture:
            fixture.stop_worker()
            fixture.stop()
        for path in root.glob('*.log'):
            shutil.copy2(path, args.output/path.name)
        shutil.rmtree(root)
        summary['fixture_removed'] = not root.exists()
        write(args.output/'summary.json', summary)


if __name__ == '__main__':
    main()
