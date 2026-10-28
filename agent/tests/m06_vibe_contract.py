#!/usr/bin/env python3
"""Exercise pinned Vibe with dynamic MCP tools and real denial callbacks."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import threading
import time

module = importlib.util.spec_from_file_location('managed_vibe', Path(__file__).parents[1] / 'vibe/runtime.py')
runtime = importlib.util.module_from_spec(module)
module.loader.exec_module(runtime)


def exercise(binary, report):
    calls, effects, versions = [], [], []
    with tempfile.TemporaryDirectory(prefix='botstral-m06-vibe-') as directory:
        root = Path(directory).resolve()
        home, workspace = root / 'home', root / 'workspace'
        workspace.mkdir()
        forbidden = root / 'outside.txt'
        names = ['manager_allowed', 'manager_denied']
        phase = ['allowed']
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_GET(self):
                self.send_error(405)
            def do_DELETE(self):
                self.send_response(204)
                self.end_headers()
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                if self.path == '/mcp':
                    assert self.headers.get('Authorization') == 'Bearer fixture-mcp'
                    method = body.get('method')
                    if method == 'initialize':
                        versions.append(body['params']['protocolVersion'])
                        result = {'protocolVersion': '2025-06-18', 'capabilities': {'tools': {}},
                                  'serverInfo': {'name': 'm06-contract', 'version': '1'}}
                    elif method == 'notifications/initialized':
                        self.send_response(202)
                        self.end_headers()
                        return
                    elif method == 'tools/list':
                        result = {'tools': [{'name': name, 'description': 'Synthetic tool.',
                                  'inputSchema': {'type': 'object', 'properties': {'value': {'type': 'string'}}, 'required': ['value']}}
                                 for name in ('allowed', 'denied')]}
                    elif method == 'tools/call':
                        effects.append(body['params'])
                        result = {'content': [{'type': 'text', 'text': 'observed-once'}]}
                    else:
                        raise AssertionError(method)
                    data = json.dumps({'jsonrpc': '2.0', 'id': body['id'], 'result': result}).encode()
                    content_type = 'application/json'
                else:
                    assert self.path == '/v1/chat/completions'
                    assert self.headers.get('Authorization') == 'Bearer fixture-provider'
                    advertised = {t['function']['name'] for t in body['tools']}
                    assert advertised == set(runtime.TOOLS) | set(names), advertised
                    calls.append(body)
                    tool = None
                    if phase[0] in ('allowed', 'denied'):
                        tool = ('manager_' + phase[0], {'value': 'fixture'})
                        phase[0] = 'done'
                    elif phase[0] == 'file':
                        tool = ('write_file', {'file_path': str(forbidden), 'content': 'forbidden'})
                        phase[0] = 'done'
                    delta = {'role': 'assistant', 'content': 'Fixture complete.'}
                    if tool:
                        delta = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': 'call-' + str(len(calls)), 'type': 'function',
                                 'function': {'name': tool[0], 'arguments': json.dumps(tool[1])}}]}
                    payload = {'id': 'fixture', 'object': 'chat.completion.chunk', 'created': 1, 'model': 'fixture',
                               'choices': [{'index': 0, 'delta': delta, 'finish_reason': 'tool_calls' if tool else 'stop'}]}
                    usage = {'id': 'fixture', 'object': 'chat.completion.chunk', 'created': 1, 'model': 'fixture', 'choices': [],
                             'usage': {'prompt_tokens': 100, 'completion_tokens': 4, 'total_tokens': 104}}
                    data = ('data: ' + json.dumps(payload) + '\n\ndata: ' + json.dumps(usage) + '\n\ndata: [DONE]\n\n').encode()
                    content_type = 'text/event-stream'
                self.send_response(200)
                self.send_header('Content-Type', content_type)
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        endpoint = f'http://127.0.0.1:{server.server_port}'
        runtime.configuration(home, workspace, endpoint + '/v1', 'fixture', 'fixture-provider')
        config = home / 'config.toml'
        config.write_text(config.read_text().replace('mcp_servers = []\n', '').replace(json.dumps(runtime.TOOLS), json.dumps(list(runtime.TOOLS) + names)) + f'''
[[mcp_servers]]
name = "manager"
transport = "streamable-http"
url = "{endpoint}/mcp"
sampling_enabled = false
[mcp_servers.auth]
type = "static"
[mcp_servers.auth.headers]
Authorization = "Bearer fixture-mcp"
''')
        profile = home / 'agents/managed.toml'
        profile.write_text(profile.read_text().replace(json.dumps(runtime.TOOLS), json.dumps(list(runtime.TOOLS) + names)) + '''
[tools.manager_allowed]
permission = "always"
[tools.manager_denied]
permission = "ask"
''')
        original_tools = runtime.TOOLS
        runtime.TOOLS = tuple(runtime.TOOLS) + tuple(names)
        try:
            version = subprocess.run([str(binary), '--version'], capture_output=True, text=True, check=True).stdout.strip()
            assert version == 'vibe 2.24.2', version
            limits = {'deadline': time.time() + 90}
            code, session = runtime.run_cli(binary, home, workspace, 'Call the allowed tool.', limits, root / 'allowed.log')
            assert code == 0 and len(effects) == 1, (code, effects, (root / 'allowed.log.stderr').read_text())
            assert 'observed-once' in json.dumps(calls[-1])
            phase[0] = 'denied'
            code, resumed = runtime.run_cli(binary, home, workspace, 'Try the denied tool.', limits, root / 'denied.log', session)
            assert code == 0 and resumed == session
            assert len(effects) == 1 and runtime.outcome(root / 'denied.log', code)[0] > 0
            phase[0] = 'file'
            code, _ = runtime.run_cli(binary, home, workspace, 'Try the outside file.', limits, root / 'file.log', session)
            assert not forbidden.exists() and runtime.outcome(root / 'file.log', code)[0] > 0
            result = {'passed': True, 'version': version, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                      'checks': ['dynamic discovery', 'Streamable HTTP negotiation', 'exact tool permission',
                                 'MCP denial callback', 'file denial callback', 'same native session'],
                      'initialization_versions_offered': versions, 'effects': len(effects), 'paid_inference': False}
            report.write_text(json.dumps(result, indent=2) + '\n')
            print(json.dumps(result))
        finally:
            runtime.TOOLS = original_tools
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--vibe', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    exercise(args.vibe.resolve(), args.report)
