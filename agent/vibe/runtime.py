import fcntl
import glob
import http.client
import importlib.util
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import secrets
import selectors
import ssl
import subprocess
import sys
import threading
import time
import uuid
from urllib.parse import urlsplit

VERSION = '2.24.2'
TOOLS = ('read_file', 'grep', 'write_file', 'edit')
MAX_REQUEST = 262144
MAX_RESPONSE = 131072
MAX_LOG = 1048576


def configuration(home, workspace, endpoint, model, token, connector_names=(), mcp_endpoint=None):
    enabled = list(TOOLS) + list(connector_names)
    home.mkdir(mode=0o700, parents=True, exist_ok=True)
    agents = home / 'agents'
    agents.mkdir(mode=0o700, exist_ok=True)
    text = f'''active_model = "managed"
default_agent = "managed"
enable_telemetry = false
enable_update_checks = false
mcp_servers = []
enabled_tools = {json.dumps(enabled)}
bypass_tool_permissions = false
[[providers]]
name = "manager"
api_base = {json.dumps(endpoint)}
api_key_env_var = ""
backend = "generic"
api_style = "openai"
[providers.extra_headers]
Authorization = {json.dumps('Bearer ' + token)}
[[models]]
name = {json.dumps(model)}
alias = "managed"
provider = "manager"
input_price = 0.0
output_price = 0.0
[compaction_model]
name = {json.dumps(model)}
alias = "managed-compact"
provider = "manager"
'''
    if mcp_endpoint:
        text = text.replace('mcp_servers = []\n', '') + f'''
[[mcp_servers]]
name = "manager"
transport = "streamable-http"
url = {json.dumps(mcp_endpoint)}
sampling_enabled = false
startup_timeout_sec = 10.0
tool_timeout_sec = 65.0
[mcp_servers.auth]
type = "static"
[mcp_servers.auth.headers]
Authorization = {json.dumps('Bearer ' + token)}
'''
    (home / 'config.toml').write_text(text)
    profile = 'display_name = "Managed file edits"\nsafety = "destructive"\nenabled_tools = ' + json.dumps(enabled) + '\nbypass_tool_permissions = false\n'
    for tool in TOOLS:
        profile += f'[tools.{tool}]\npermission = "ask"\nallowlist = {json.dumps([glob.escape(str(workspace)) + "/*"])}\n'
    for tool in connector_names:
        profile += f'[tools.{tool}]\npermission = "always"\n'
    (agents / 'managed.toml').write_text(profile)
    (home / 'hooks.toml').write_text('')
    for path in (home / 'config.toml', agents / 'managed.toml', home / 'hooks.toml'):
        path.chmod(0o600)


def native_session(home):
    files = list((home / 'logs' / 'session').glob('*/meta.json'))
    if len(files) != 1 or files[0].is_symlink() or files[0].stat().st_size > MAX_LOG:
        raise ValueError('exactly one bounded native session is required')
    metadata = json.loads(files[0].read_text())
    session = metadata['session_id']
    if str(uuid.UUID(session)) != session:
        raise ValueError('native session identity')
    return session


def run_cli(binary, home, workspace, prompt, limits, output, resume=None, connector_names=(), stopped=lambda: False):
    environment = {'PATH': '/usr/bin:/bin', 'HOME': str(home), 'VIBE_HOME': str(home),
                   'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8', 'LOG_LEVEL': 'ERROR', 'PYTHONDONTWRITEBYTECODE': '1'}
    if resume and native_session(home) != resume:
        raise ValueError('resume session mismatch')
    argv = [str(binary), '--prompt', prompt, '--output', 'json', '--agent', 'managed',
            '--workdir', str(workspace)]
    for tool in (*TOOLS, *connector_names):
        argv += ['--enabled-tools', tool]
    if resume:
        argv += ['--resume', resume]
    child = subprocess.Popen(argv, env=environment, cwd=workspace, stdin=subprocess.DEVNULL,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    total = 0
    with selectors.DefaultSelector() as ready, open(output, 'xb') as log, open(str(output) + '.stderr', 'xb') as errors:
        for stream in (child.stdout, child.stderr):
            os.set_blocking(stream.fileno(), False)
            ready.register(stream, selectors.EVENT_READ)
        while ready.get_map():
            if time.time() >= limits['deadline'] or stopped():
                child.terminate()
                try:
                    child.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=1)
                raise TimeoutError('coding stopped')
            for key, _ in ready.select(.1):
                block = os.read(key.fd, 16384)
                if not block:
                    ready.unregister(key.fileobj)
                    continue
                total += len(block)
                if total > MAX_LOG:
                    child.terminate()
                    raise ValueError('coding output bound')
                (log if key.fileobj is child.stdout else errors).write(block)
        code = child.wait(timeout=2)
        log.flush()
        os.fsync(log.fileno())
    return code, native_session(home)


def outcome(output, code):
    history = json.loads(output.read_text()) if code == 0 else []
    denied = sum(entry.get('type') == 'callback' and entry.get('state', {}).get('output', {}).get('decision', {}).get('type') == 'deny' for entry in history)
    errors = sum(entry.get('type') == 'effect' and entry.get('state', {}).get('status') in ('failed', 'cancelled') for entry in history)
    return denied, errors


class Relay(HTTPServer):
    def __init__(self, launch, transport):
        self.launch = launch
        self.transport = transport
        self.token = secrets.token_urlsafe(32)
        self.failed = False
        self.calls = 0
        self.sessions = {}
        module = importlib.util.spec_from_file_location('connector_runtime', Path(launch['directory']) / 'connector-runtime.py')
        self.connector_runtime = importlib.util.module_from_spec(module)
        module.loader.exec_module(self.connector_runtime)
        self.connectors = self.connector_runtime.Gateway(launch, transport)
        super().__init__(('127.0.0.1', 0), Handler)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.send_error(405)

    def do_DELETE(self):
        self.send_error(405)

    def do_POST(self):
        s = self.server
        if self.path == '/mcp':
            self.mcp()
            return
        self.connection.settimeout(125)
        if self.path != '/v1/chat/completions' or self.headers.get('Authorization') != 'Bearer ' + s.token:
            self.send_error(403)
            return
        length = int(self.headers.get('Content-Length', '0'))
        if s.failed or not 0 < length <= MAX_REQUEST:
            self.send_error(409)
            return
        body = self.rfile.read(length)
        if len(body) != length:
            self.send_error(400)
            return
        transport = s.transport
        target = urlsplit(transport['manager'])
        if target.scheme != 'wss' or target.path not in ('', '/'):
            self.send_error(500)
            return
        pin = transport['pin']
        if type(pin) is not str or len(pin) != 64 or any(c not in '0123456789abcdef' for c in pin):
            self.send_error(500)
            return
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
        context.load_cert_chain(transport['client_cert'], transport['client_key'])
        call = str(uuid.uuid4())
        connection = http.client.HTTPSConnection(target.hostname, target.port, context=context, timeout=120)
        s.calls += 1
        try:
            path = '/worker/v1/coding/' + s.launch['task'] + '/' + s.launch['execution'] + '/' + call
            connection.request('POST', path, body, {'Content-Type': 'application/json', 'Connection': 'close'})
            response = connection.getresponse()
            data = response.read(MAX_RESPONSE + 1)
            if len(data) > MAX_RESPONSE:
                raise ValueError('coding response bound')
            if response.status != 200:
                s.failed = True
            self.send_response(response.status)
            self.send_header('Content-Type', response.getheader('Content-Type', 'application/json'))
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except (OSError, ValueError, http.client.HTTPException):
            s.failed = True
            self.send_error(502)
        finally:
            connection.close()


    def mcp(self):
        s = self.server
        if s.failed or self.headers.get('Authorization') != 'Bearer ' + s.token or self.headers.get('Origin'):
            self.send_error(403)
            return
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= MAX_REQUEST:
                raise ValueError('MCP request bound')
            request = json.loads(self.rfile.read(length))
            if request.get('jsonrpc') != '2.0':
                raise ValueError('MCP request')
            method = request.get('method')
            identity = request.get('id')
            session = self.headers.get('Mcp-Session-Id')
            if method == 'initialize':
                if len(s.sessions) >= 64:
                    raise ValueError('MCP session bound')
                session = str(uuid.uuid4())
                s.sessions[session] = False
                result = {'protocolVersion': s.connector_runtime.VERSION, 'capabilities': {'tools': {}},
                          'serverInfo': {'name': 'botstral', 'version': '1'}}
            else:
                if session not in s.sessions or self.headers.get('MCP-Protocol-Version') != s.connector_runtime.VERSION:
                    raise ValueError('MCP session')
                if method == 'notifications/initialized':
                    s.sessions[session] = True
                    self.send_response(202)
                    self.end_headers()
                    return
                if not s.sessions[session]:
                    raise ValueError('MCP initialization')
                if method == 'ping':
                    result = {}
                elif method == 'tools/list':
                    definitions = s.connectors.catalog()
                    result = {'tools': [{'name': s.connector_runtime.public_name(t['binding']), 'description': t['description'],
                                         'inputSchema': t['input_schema']} for t in definitions]}
                elif method == 'tools/call':
                    params = request['params']
                    definitions = s.connectors.catalog()
                    definition = next((t for t in definitions if s.connector_runtime.public_name(t['binding']) == params['name']), None)
                    if definition is None:
                        raise ValueError('MCP tool authority')
                    invocation = str(uuid.uuid5(uuid.UUID(session), json.dumps(identity)))
                    call = s.connectors.execute(invocation, {'type': 'call', 'proposal': {'tool': definition['binding'], 'arguments': params.get('arguments', {})}})
                    outcome = call['state']['outcome']
                    if outcome['type'] == 'finished':
                        result = outcome['result']['value']
                    else:
                        result = {'isError': True, 'content': [{'type': 'text', 'text': 'Connector request was not sent.'}]}
                else:
                    self.mcp_reply(session, {'jsonrpc': '2.0', 'id': identity, 'error': {'code': -32601, 'message': 'Unsupported method'}})
                    return
            self.mcp_reply(session, {'jsonrpc': '2.0', 'id': identity, 'result': result})
        except Exception:
            s.failed = True
            self.send_error(409)

    def mcp_reply(self, session, value):
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Mcp-Session-Id', session)
        self.end_headers()
        self.wfile.write(data)


def execute(path):
    os.umask(0o077)
    launch = json.loads(Path(path).read_text())
    spec = launch['spec']
    workspace = Path(spec['policy']['workspace']['path'])
    if str(workspace.resolve(strict=True)) != str(workspace) or not workspace.is_dir():
        raise ValueError('workspace must be an existing canonical directory')
    home = Path(launch['home'])
    if home.is_symlink():
        raise ValueError('home is not a directory')
    new = spec['session']['type'] == 'new'
    if new:
        home.mkdir(mode=0o700)
    elif not home.is_dir():
        raise ValueError('resume home missing')
    lock = open(home / 'writer.lock', 'a')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    transport = json.loads(Path(launch['transport']).read_text())
    relay = Relay(launch, transport)
    thread = threading.Thread(target=relay.serve_forever, daemon=True)
    thread.start()
    output = Path(launch['outputs'])
    output.mkdir(mode=0o700)
    policy = spec['policy'].get('connectors')
    connector_names = tuple('manager_' + relay.connector_runtime.public_name(t) for t in policy['tools']) if policy else ()
    configuration(home, workspace, f'http://127.0.0.1:{relay.server_port}/v1', spec['policy']['model_id'], relay.token,
                  connector_names, f'http://127.0.0.1:{relay.server_port}/mcp' if policy else None)
    try:
        code, session = run_cli(Path(launch['runtime']) / 'bin/vibe', home, workspace,
                                spec['prompt'], spec['limits'], output / 'transcript.out',
                                None if new else spec['session']['native_session'], connector_names, lambda: relay.failed or relay.connectors.failed)
        for declaration in spec['policy']['outputs']:
            slot = declaration['slot']
            if slot == 'transcript':
                continue
            source = workspace / declaration['name']
            if source.is_symlink() or not source.is_file():
                continue
            with source.open('rb') as stream:
                data = stream.read(declaration['max_bytes'] + 1)
            if len(data) <= declaration['max_bytes']:
                (output / (slot + '.out')).write_bytes(data)
        denied, failed = outcome(output / 'transcript.out', code)
        print(json.dumps({'native_session': session, 'exit_code': code, 'calls': relay.calls,
                          'approval_denied': denied, 'tool_errors': failed}))
        return code or int(bool(denied or failed or relay.failed))
    finally:
        relay.shutdown()
        relay.server_close()
        lock.close()


if __name__ == '__main__':
    try:
        sys.exit(execute(sys.argv[1]))
    except Exception:
        print('coding runtime failed', file=sys.stderr)
        sys.exit(1)
