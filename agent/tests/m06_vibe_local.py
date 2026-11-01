#!/usr/bin/env python3
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import shutil
import ssl
import subprocess
import tempfile
import threading
import time
import uuid

from m06_connector_contract import permit, runtime as connector_runtime

module = importlib.util.spec_from_file_location('managed_vibe', Path(__file__).parents[1] / 'vibe/runtime.py')
runtime = importlib.util.module_from_spec(module)
module.loader.exec_module(runtime)


def exercise(binary, report):
    evidence = []
    for mode in ('normal', 'lost'):
        with tempfile.TemporaryDirectory(prefix='botstral-m06-vibe-local-') as directory:
            root = Path(directory).resolve()
            cert, key = root / 'cert.pem', root / 'key.pem'
            subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(key), '-out', str(cert),
                            '-days', '1', '-subj', '/CN=localhost', '-addext', 'subjectAltName=IP:127.0.0.1',
                            '-addext', 'extendedKeyUsage=serverAuth,clientAuth'], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            key.chmod(0o600)
            task, execution = str(uuid.uuid4()), str(uuid.uuid4())
            local = permit(root, mode)
            binding = local['operation']['proposal']['tool']
            name = connector_runtime.public_name(binding)
            definition = {'binding': binding, 'description': 'Synthetic stdio tool', 'input_schema': {'type': 'object', 'properties': {'value': {'type': 'string'}}, 'required': ['value'], 'additionalProperties': False}}
            events, inferences, ledger = [], [], {}
            workspace = root / 'workspace'
            workspace.mkdir()
            outside = root / 'outside.txt'
            class Manager(BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass
                def do_POST(self):
                    assert self.connection.getpeercert()
                    value = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    if self.path == f'/worker/v1/connectors/{task}/{execution}':
                        events.append(value['type'])
                        if value['type'] == 'catalog':
                            result = {'type': 'catalog', 'tools': [definition]}
                        elif value['type'] == 'execute':
                            identity = value['id']
                            assert identity not in ledger
                            assert value['operation']['proposal']['tool'] == binding
                            ledger[identity] = {'id': identity, 'state': {'type': 'dispatch_possible'}}
                            result = dict(local, id=identity, operation=value['operation'])
                        elif value['type'] == 'status':
                            result = {'type': 'status', 'call': ledger[value['id']]}
                        elif value['type'] == 'settle':
                            assert 'synthetic-local-private-token' not in json.dumps(value)
                            ledger[value['id']]['state'] = {'type': 'resolved', 'outcome': value['outcome']}
                            result = {'type': 'status', 'call': ledger[value['id']]}
                        else:
                            raise AssertionError(value['type'])
                        data = json.dumps(result).encode()
                        content_type = 'application/json'
                    else:
                        assert self.path.startswith(f'/worker/v1/coding/{task}/{execution}/')
                        assert {t['function']['name'] for t in value['tools']} == set(runtime.TOOLS) | {'manager_' + name}
                        inferences.append(value)
                        index = len(inferences)
                        delta = {'role': 'assistant', 'content': 'Fixture complete.'}
                        if index <= 2:
                            tool = ('manager_' + name, {'value': 'hello'}) if index == 1 else ('write_file', {'file_path': str(outside), 'content': 'forbidden'})
                            delta = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': 'tool-' + str(index), 'type': 'function', 'function': {'name': tool[0], 'arguments': json.dumps(tool[1])}}]}
                        payload = {'id': 'fixture', 'object': 'chat.completion.chunk', 'created': 1, 'model': 'fixture',
                                   'choices': [{'index': 0, 'delta': delta, 'finish_reason': 'tool_calls' if index <= 2 else 'stop'}]}
                        usage = {'id': 'fixture', 'object': 'chat.completion.chunk', 'created': 1, 'model': 'fixture', 'choices': [],
                                 'usage': {'prompt_tokens': 100, 'completion_tokens': 4, 'total_tokens': 104}}
                        data = ('data: ' + json.dumps(payload) + '\n\ndata: ' + json.dumps(usage) + '\n\ndata: [DONE]\n\n').encode()
                        content_type = 'text/event-stream'
                    self.send_response(200)
                    self.send_header('Content-Type', content_type)
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
            shutil.copyfile(Path(__file__).parents[1] / 'connector/runtime.py', root / 'connector-runtime.py')
            launch = {'directory': str(root), 'task': task, 'execution': execution}
            transport = {'manager': f'wss://127.0.0.1:{manager.server_port}', 'pin': '0' * 64, 'client_cert': str(cert), 'client_key': str(key)}
            relay = runtime.Relay(launch, transport)
            threading.Thread(target=relay.serve_forever, daemon=True).start()
            home = root / 'home'
            names = ('manager_' + name,)
            endpoint = f'http://127.0.0.1:{relay.server_port}'
            runtime.configuration(home, workspace, endpoint + '/v1', 'fixture', relay.token, names, endpoint + '/mcp')
            try:
                try:
                    code, _ = runtime.run_cli(binary, home, workspace, 'Call the connector and try the proposed file.',
                                              {'deadline': time.time() + 60}, root / 'transcript.json', connector_names=names,
                                              stopped=lambda: relay.failed or relay.connectors.failed)
                    assert mode == 'normal' and code == 0
                    assert runtime.outcome(root / 'transcript.json', code)[0] > 0
                except TimeoutError:
                    assert mode == 'lost' and (relay.failed or relay.connectors.failed)
                assert not outside.exists()
                effects = [json.loads(line) for line in (root / 'effects.jsonl').read_text().splitlines()]
                assert len(effects) == 1 and effects[0]['parent'] == os.getpid()
                if effects[0]['cgroup'] is not None:
                    assert effects[0]['cgroup'] == Path('/proc/self/cgroup').read_text()
                assert events.count('execute') == 1
                if mode == 'lost':
                    assert len(inferences) == 1
                assert 'synthetic-local-private-token' not in json.dumps(inferences)
                evidence.append({'mode': mode, 'effects': len(effects), 'same_wrapper_parent': True,
                                 'same_cgroup': effects[0]['cgroup'] is not None, 'inferences': len(inferences),
                                 'gateway_operations': events, 'outside_file_denied': True})
            finally:
                relay.shutdown()
                relay.server_close()
                manager.shutdown()
                manager.server_close()
    report.write_text(json.dumps({'passed': True, 'vibe': '2.24.2', 'paid_inference': False, 'runs': evidence,
                                 'systemd_scope_acceptance': 'Required on Linux; this fixture does not create a systemd scope.'}, indent=2) + '\n')
    print(report.read_text())


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--vibe', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    exercise(args.vibe.resolve(), args.report)
