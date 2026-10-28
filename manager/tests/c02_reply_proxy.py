#!/usr/bin/env python3
"""Drop one real SubmitRun response after the actual manager commits it."""
import json
import select
import socket
import ssl
import struct
import threading


def exact(connection, count):
    data = b''
    while len(data) < count:
        part = connection.recv(count-len(data))
        if not part:
            raise EOFError()
        data += part
    return data


def frame(connection):
    head = exact(connection, 2)
    size = head[1] & 127
    if size == 126:
        extension = exact(connection, 2)
        size = struct.unpack('!H', extension)[0]
        head += extension
    elif size == 127:
        extension = exact(connection, 8)
        size = struct.unpack('!Q', extension)[0]
        head += extension
    if size > 1024*1024:
        raise ValueError('Fixture frame limit')
    mask = exact(connection, 4) if head[1] & 128 else b''
    body = exact(connection, size)
    plain = bytes(v ^ mask[i % 4] for i, v in enumerate(body)) if mask else body
    value = json.loads(plain) if head[0] & 15 == 1 else {}
    return head+mask+body, value


class ReplyProxy:
    def __init__(self, root, endpoint):
        self.root, self.endpoint = root, endpoint
        self.stopped = threading.Event()
        self.armed = threading.Event()
        self.observed = dict(submissions=0, dropped_replies=0)
        self.listener = socket.socket()
        self.listener.bind(('127.0.0.1', 0))
        self.listener.listen()
        self.listener.settimeout(.2)
        self.address = '127.0.0.1:'+str(self.listener.getsockname()[1])
        self.connections = []
        self.threads = []
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        while not self.stopped.is_set():
            try:
                raw, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            thread = threading.Thread(target=self.peer, args=(raw,), daemon=True)
            self.threads.append(thread)
            thread.start()

    def peer(self, raw):
        server = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        server.load_cert_chain(self.root/'manager.pem', self.root/'manager.key')
        backend = None
        client = None
        try:
            client = server.wrap_socket(raw, server_side=True)
            host, port = self.endpoint.rsplit(':', 1)
            context = ssl.create_default_context(cafile=str(self.root/'ca.pem'))
            backend = context.wrap_socket(socket.create_connection((host, int(port)), timeout=10), server_hostname='192.168.1.40')
            self.connections.extend([client, backend])
            for source, destination in [(client, backend), (backend, client)]:
                head = b''
                while not head.endswith(b'\r\n\r\n'):
                    head += exact(source, 1)
                destination.sendall(head)
            target = None
            while not self.stopped.is_set():
                ready = [s for s in (client, backend) if s.pending()]
                if not ready:
                    ready, _, _ = select.select([client, backend], [], [], .2)
                for source in ready:
                    wire, value = frame(source)
                    if source is client:
                        operation = value.get('operation', {})
                        if operation.get('method') == 'submit_run':
                            self.observed['submissions'] += 1
                            if self.armed.is_set():
                                target = value['id']
                                self.armed.clear()
                        backend.sendall(wire)
                    else:
                        if target is not None and value.get('request') == target and value.get('type') == 'transfer_start':
                            self.observed['dropped_replies'] += 1
                            return
                        client.sendall(wire)
        except (OSError, EOFError, ValueError):
            pass
        finally:
            for connection in (client, backend, raw):
                if connection:
                    connection.close()

    def close(self):
        self.stopped.set()
        self.listener.close()
        for connection in self.connections:
            try:
                connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            connection.close()
        self.thread.join(timeout=5)
        for thread in self.threads:
            thread.join(timeout=5)
        if self.thread.is_alive() or any(t.is_alive() for t in self.threads):
            raise RuntimeError('Reply proxy did not stop')
