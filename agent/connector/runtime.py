"""Task-bound connector gateway and bounded ephemeral MCP stdio client."""
import hashlib
import http.client
import json
import os
from pathlib import Path
import selectors
import signal
import ssl
import struct
import subprocess
import sys
import time
import uuid
from urllib.parse import urlsplit

VERSION = '2025-06-18'
MAX_FRAME = 131072
MAX_CATALOG = 32768
MAX_TOOLS = 32


class ConnectorError(Exception):
    def __init__(self, reason, possible=False):
        self.reason, self.possible = reason, possible
        super().__init__(reason)


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def digest(value):
    result = hashlib.sha256()
    def length(n):
        result.update(struct.pack('>Q', n))
    def add(v):
        if v is None:
            result.update(b'n')
        elif isinstance(v, bool):
            result.update(b't' if v else b'f')
        elif isinstance(v, str):
            data = v.encode(); result.update(b's'); length(len(data)); result.update(data)
        elif isinstance(v, int) and -(1 << 63) <= v < (1 << 64):
            data = str(v).encode(); result.update(b'i'); length(len(data)); result.update(data)
        elif isinstance(v, (int, float)):
            result.update(b'd'); result.update(struct.pack('>d', float(v)))
        elif isinstance(v, list):
            result.update(b'a'); length(len(v))
            for item in v:
                add(item)
        elif isinstance(v, dict):
            result.update(b'o'); length(len(v))
            for key in sorted(v):
                add(key); add(v[key])
        else:
            raise ValueError('JSON value required')
    add(value)
    return result.hexdigest()


def public_name(binding):
    return 'connector_' + digest(binding)[:40]


def durable(path, value, new=False):
    data = encoded(value)
    temporary = path.with_suffix('.tmp')
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
    descriptor = os.open(temporary, flags, 0o600)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        if new:
            os.link(temporary, path)
            temporary.unlink()
        else:
            os.replace(temporary, path)
        descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    finally:
        if temporary.exists():
            temporary.unlink()


def redact(value, secrets):
    if isinstance(value, str):
        for secret in secrets:
            if secret:
                value = value.replace(secret, '[redacted]')
        return value
    if isinstance(value, list):
        return [redact(v, secrets) for v in value]
    if isinstance(value, dict):
        return {redact(k, secrets): redact(v, secrets) for k, v in value.items()}
    return value


class Stdio:
    def __init__(self, permit, alive=lambda: True, before_call=lambda: True):
        self.permit = permit
        self.alive = alive
        self.before_call = before_call
        self.deadline = min(permit['deadline'], time.time() + 60)
        self.sent = False
        self.total = 0
        self.changed = False
        self.child = None
        self.ready = selectors.DefaultSelector()
        self.buffer = b''

    def check(self):
        if time.time() >= self.deadline:
            raise ConnectorError('deadline', self.sent)
        if not self.alive():
            raise ConnectorError('authority', True)

    def write(self, value):
        data = encoded(value) + b'\n'
        if len(data) > MAX_FRAME:
            raise ConnectorError('bound', self.sent)
        remaining = memoryview(data)
        while remaining:
            self.check()
            try:
                count = os.write(self.child.stdin.fileno(), remaining)
                remaining = remaining[count:]
            except BlockingIOError:
                time.sleep(.01)
            except OSError:
                raise ConnectorError('transport', self.sent) from None

    def read(self):
        while True:
            self.check()
            if b'\n' in self.buffer:
                line, self.buffer = self.buffer.split(b'\n', 1)
                try:
                    return json.loads(line)
                except (ValueError, UnicodeError):
                    raise ConnectorError('protocol', self.sent) from None
            if not self.ready.select(.1):
                continue
            block = os.read(self.child.stdout.fileno(), 16384)
            if not block:
                raise ConnectorError('transport', self.sent)
            self.total += len(block)
            self.buffer += block
            if len(self.buffer) > MAX_FRAME or self.total > MAX_FRAME * 2:
                raise ConnectorError('bound', self.sent)

    def rpc(self, identity, method, params):
        self.write({'jsonrpc': '2.0', 'id': identity, 'method': method, 'params': params})
        for _ in range(64):
            value = self.read()
            if not isinstance(value, dict) or value.get('jsonrpc') != '2.0':
                raise ConnectorError('protocol', self.sent)
            if 'method' in value:
                if value['method'] == 'notifications/tools/list_changed':
                    self.changed = True
                if 'id' in value:
                    response = {'jsonrpc': '2.0', 'id': value['id']}
                    if value['method'] == 'ping':
                        response['result'] = {}
                    else:
                        response['error'] = {'code': -32601, 'message': 'Unsupported client capability'}
                    self.write(response)
                continue
            if value.get('id') != identity or ('result' in value) == ('error' in value):
                raise ConnectorError('protocol', self.sent)
            return value
        raise ConnectorError('bound', self.sent)

    def tools(self):
        tools, cursors = [], set()
        cursor = None
        for page in range(8):
            result = self.rpc('list_' + str(page), 'tools/list', {} if cursor is None else {'cursor': cursor}).get('result', {})
            if not isinstance(result.get('tools'), list):
                raise ConnectorError('protocol', self.sent)
            for tool in result['tools']:
                if not isinstance(tool, dict) or not isinstance(tool.get('name'), str) or not isinstance(tool.get('inputSchema'), dict):
                    raise ConnectorError('protocol', self.sent)
                tools.append({'name': tool['name'], 'description': tool.get('description', ''), 'input_schema': tool['inputSchema']})
            if len(tools) > MAX_TOOLS or len(encoded(tools)) > MAX_CATALOG or len({t['name'] for t in tools}) != len(tools):
                raise ConnectorError('bound', self.sent)
            if 'nextCursor' not in result:
                return tools
            cursor = result['nextCursor']
            if not isinstance(cursor, str) or not 0 < len(cursor) <= 512 or cursor in cursors:
                raise ConnectorError('protocol', self.sent)
            cursors.add(cursor)
        raise ConnectorError('bound', self.sent)

    def execute(self):
        config = self.permit['transport']
        if config['type'] != 'worker_stdio':
            raise ConnectorError('protocol')
        environment = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8'}
        environment.update(config['environment'])
        secret = self.permit['secret']
        secrets = secret.get('values', {}) if secret['type'] == 'local_environment' else {}
        environment.update(secrets)
        try:
            self.check()
            self.child = subprocess.Popen([config['executable'], *config['arguments']], cwd=config['cwd'], env=environment,
                                          stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                          close_fds=True, start_new_session=True)
            os.set_blocking(self.child.stdin.fileno(), False)
            os.set_blocking(self.child.stdout.fileno(), False)
            self.ready.register(self.child.stdout, selectors.EVENT_READ)
            result = self.rpc('initialize', 'initialize', {'protocolVersion': VERSION, 'capabilities': {},
                              'clientInfo': {'name': 'botstral', 'version': '1'}}).get('result', {})
            if result.get('protocolVersion') != VERSION or not isinstance(result.get('capabilities', {}).get('tools'), dict):
                raise ConnectorError('protocol')
            self.write({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
            tools = self.tools()
            operation = self.permit['operation']
            if operation['type'] == 'discover':
                result = {'type': 'catalog', 'tools': tools}
            else:
                proposal = operation['proposal']
                binding = proposal['tool']
                tool = next((t for t in tools if t['name'] == binding['name']), None)
                if tool is None or self.changed or digest(tool['input_schema']) != binding['schema_digest']:
                    raise ConnectorError('catalog_changed')
                self.check()
                if not self.before_call():
                    raise ConnectorError('authority')
                self.sent = True
                response = self.rpc(self.permit['id'], 'tools/call', {'name': binding['name'], 'arguments': proposal['arguments']})
                value = response.get('result', {'isError': True, 'content': [{'type': 'text', 'text': 'Connector returned a protocol error.'}]})
                if not isinstance(value, dict) or not isinstance(value.get('content'), list) or len(encoded(value)) > self.permit['max_result_bytes']:
                    raise ConnectorError('bound', True)
                result = {'type': 'tool', 'value': value}
            return {'type': 'finished', 'result': redact(result, secrets.values())}
        except ConnectorError as error:
            return {'type': 'unknown' if error.possible else 'not_sent', 'reason': error.reason}
        except (OSError, ValueError, TypeError, KeyError):
            return {'type': 'unknown' if self.sent else 'not_sent', 'reason': 'transport'}
        finally:
            self.ready.close()
            if self.child:
                self.child.stdin.close()
                try:
                    self.child.wait(timeout=.25)
                except subprocess.TimeoutExpired:
                    self.child.terminate()
                    try:
                        self.child.wait(timeout=.25)
                    except subprocess.TimeoutExpired:
                        self.child.kill()
                        self.child.wait(timeout=1)
                try:
                    os.killpg(self.child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                self.child.stdout.close()


class Gateway:
    def __init__(self, launch, transport):
        self.launch, self.transport = launch, transport
        self.directory = Path(launch['directory']) / 'connector-calls'
        self.directory.mkdir(mode=0o700, exist_ok=True)
        self.failed = False

    def request(self, body):
        target = urlsplit(self.transport['manager'])
        if target.scheme != 'wss' or target.path not in ('', '/'):
            raise ConnectorError('protocol')
        context = ssl.create_default_context(cafile=self.transport['manager_ca'])
        context.load_cert_chain(self.transport['client_cert'], self.transport['client_key'])
        connection = http.client.HTTPSConnection(target.hostname, target.port, context=context, timeout=65 if body['type'] == 'execute' else 5)
        try:
            path = '/worker/v1/connectors/' + self.launch['task'] + '/' + self.launch['execution']
            connection.request('POST', path, encoded(body), {'Content-Type': 'application/json', 'Connection': 'close'})
            response = connection.getresponse()
            data = response.read(MAX_FRAME + 1)
            if response.status != 200 or len(data) > MAX_FRAME:
                raise ConnectorError('authority')
            return json.loads(data)
        except (OSError, ValueError, http.client.HTTPException):
            raise ConnectorError('transport', True) from None
        finally:
            connection.close()

    def catalog(self):
        if self.failed:
            raise ConnectorError('interrupted', True)
        response = self.request({'type': 'catalog'})
        if response.get('type') != 'catalog':
            raise ConnectorError('protocol')
        return response['tools']

    def execute(self, identity, operation):
        identity = str(uuid.UUID(identity))
        path = self.directory / (identity + '.json')
        operation_digest = digest(operation)
        if self.failed:
            raise ConnectorError('interrupted', True)
        try:
            if path.exists():
                saved = json.loads(path.read_bytes())
                if saved['digest'] != operation_digest:
                    raise ConnectorError('protocol', True)
                if 'outcome' in saved:
                    response = self.request({'type': 'settle', 'id': identity, 'outcome': saved['outcome']})
                else:
                    response = self.request({'type': 'status', 'id': identity})
            else:
                durable(path, {'digest': operation_digest, 'phase': 'requested'}, new=True)
                response = self.request({'type': 'execute', 'id': identity, 'operation': operation})
            if response['type'] == 'local':
                durable(path, {'digest': operation_digest, 'phase': 'dispatch_possible'})
                next_check = [0]
                def current_authority():
                    status = self.request({'type': 'status', 'id': identity})
                    return status['call']['state']['type'] == 'dispatch_possible'
                def alive():
                    if time.monotonic() < next_check[0]:
                        return True
                    next_check[0] = time.monotonic() + .25
                    return current_authority()
                outcome = Stdio(response, alive, current_authority).execute()
                durable(path, {'digest': operation_digest, 'phase': 'captured', 'outcome': outcome})
                response = self.request({'type': 'settle', 'id': identity, 'outcome': outcome})
            call = response['call']
            state = call['state']
            if state['type'] != 'resolved' or state['outcome']['type'] == 'unknown':
                raise ConnectorError('interrupted', True)
            durable(path.with_suffix('.ack'), True)
            return call
        except (ConnectorError, OSError, ValueError, KeyError):
            self.failed = True
            raise ConnectorError('interrupted', True) from None


def execute(path):
    os.umask(0o077)
    launch = json.loads(Path(path).read_bytes())
    transport = json.loads(Path(launch['transport']).read_bytes())
    gateway = Gateway(launch, transport)
    identity = str(uuid.uuid5(uuid.UUID(launch['execution']), 'connector'))
    call = gateway.execute(identity, launch['spec']['operation'])
    output = Path(launch['outputs'])
    output.mkdir(mode=0o700)
    durable(output / 'connector.out', {'call': call['id']}, new=True)
    return int(call['state']['outcome']['type'] != 'finished')


if __name__ == '__main__':
    try:
        sys.exit(execute(sys.argv[1]))
    except Exception:
        print('connector outcome requires attention', file=sys.stderr)
        sys.exit(1)
