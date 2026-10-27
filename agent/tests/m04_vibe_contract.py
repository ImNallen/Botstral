#!/usr/bin/env python3
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import resource
import sys
import threading
import time

module = importlib.util.spec_from_file_location('managed_vibe', Path(__file__).parents[1] / 'vibe/runtime.py')
runtime = importlib.util.module_from_spec(module)
module.loader.exec_module(runtime)


def exercise(binary, report, wire_report=None):
    calls = []
    with tempfile.TemporaryDirectory(prefix='botstral-m04-vibe-') as directory:
        root = Path(directory).resolve()
        workspace = root / 'workspace[12]'
        workspace.mkdir()
        home = root / 'home'
        target = workspace / 'answer.py'
        target.write_text('def answer():\n    return 0\n')
        forbidden = root / 'workspace1' / 'not-approved.txt'
        forbidden.parent.mkdir()
        phase = ['edit']
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                assert self.path == '/v1/chat/completions'
                assert self.headers.get('Authorization') == 'Bearer fixture-local-only'
                assert body['model'] == 'fixture-pinned'
                assert body['stream'] is True
                assert body['stream_options']['include_usage'] is True
                assert {t['function']['name'] for t in body['tools']} == set(runtime.TOOLS)
                calls.append(body)
                tool = None
                if phase[0] == 'edit':
                    tool = ('edit', {'file_path': str(target), 'old_string': 'return 0', 'new_string': 'return 42'})
                    phase[0] = 'edited'
                elif phase[0] == 'deny':
                    tool = ('write_file', {'file_path': str(forbidden), 'content': 'forbidden'})
                    phase[0] = 'denied'
                elif phase[0] == 'bash':
                    tool = ('bash', {'command': 'touch ' + str(forbidden)})
                    phase[0] = 'bash-denied'
                delta = {'role': 'assistant', 'content': 'Fixture complete.'}
                if tool:
                    delta = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': 'fixture-call-' + str(len(calls)), 'type': 'function',
                              'function': {'name': tool[0], 'arguments': json.dumps(tool[1])}}]}
                payload = {'id': 'fixture-response', 'object': 'chat.completion.chunk', 'created': 1,
                           'model': 'fixture-pinned', 'choices': [{'index': 0, 'delta': delta,
                           'finish_reason': 'tool_calls' if tool else 'stop'}]}
                usage = {'id': 'fixture-response', 'object': 'chat.completion.chunk', 'created': 1,
                         'model': 'fixture-pinned', 'choices': [],
                         'usage': {'prompt_tokens': 100, 'completion_tokens': 4, 'total_tokens': 104}}
                data = ('data: ' + json.dumps(payload) + '\n\ndata: ' + json.dumps(usage) + '\n\ndata: [DONE]\n\n').encode()
                self.send_response(200)
                self.send_header('Content-Type', 'text/event-stream')
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        runtime.configuration(home, workspace, f'http://127.0.0.1:{server.server_port}/v1', 'fixture-pinned', 'fixture-local-only')
        limits = {'max_turns': 4, 'max_total_tokens': 20000, 'deadline': int(time.time()) + 90}
        try:
            version = subprocess.run([str(binary), '--version'], env={'PATH':'/usr/bin:/bin','HOME':str(home),'VIBE_HOME':str(home),'PYTHONDONTWRITEBYTECODE':'1'}, capture_output=True, text=True, check=True).stdout.strip()
            assert runtime.VERSION in version, version
            code, session = runtime.run_cli(binary, home, workspace, 'Fix answer.py.', limits, root / 'first.log')
            assert code == 0, (root / 'first.log').read_text()[-2000:]
            assert target.read_text() == 'def answer():\n    return 42\n', (root / 'first.log').read_text()[-4000:]
            assert len(calls) == 2
            assert any(m.get('role') == 'tool' for m in calls[-1]['messages'])
            subprocess.run(['/usr/bin/python3', '-c', 'from answer import answer; assert answer() == 42'], cwd=workspace, check=True)
            phase[0] = 'deny'
            code, resumed = runtime.run_cli(binary, home, workspace, 'Try the requested edit.', limits, root / 'resume.log', session)
            assert code == 0 and resumed == session, (code, session, resumed, (root / 'resume.log').read_text()[-3000:])
            assert not forbidden.exists()
            assert '"type": "deny"' in (root / 'resume.log').read_text()
            denied, errors = runtime.outcome(root / 'resume.log', code)
            assert denied > 0 and errors > 0, (denied, errors)
            phase[0] = 'bash'
            code, resumed = runtime.run_cli(binary, home, workspace, 'Try the requested tool.', limits, root / 'bash.log', session)
            assert resumed == session and not forbidden.exists()
            assert 'bash' in ((root / 'bash.log').read_text() + (root / 'bash.log.stderr').read_text())
            assert code in (0, 1)
            assert len(calls) >= 4
            result = {'passed': True, 'lane': 'recorded-actual-vibe', 'version': version,
                      'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                      'runtime_script_sha256': hashlib.sha256(Path(runtime.__file__).read_bytes()).hexdigest(),
                      'calls': len(calls), 'tools': list(runtime.TOOLS), 'native_session': session,
                      'checks': ['real edit', 'separate Python test', 'explicit same-session resume',
                                 'outside-path callback denied', 'bash unavailable', 'SSE usage'],
                      'live_acceptance': False}
            result['max_child_rss'] = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
            result['max_child_rss_units'] = 'KiB' if sys.platform == 'linux' else 'bytes'
            if sys.platform == 'linux':
                cgroup = Path('/sys/fs/cgroup') / Path('/proc/self/cgroup').read_text().strip().split('::', 1)[1].lstrip('/')
                result['cgroup'] = {name: (cgroup / name).read_text().strip() for name in ('memory.peak', 'memory.max', 'memory.swap.max', 'pids.max')}
                result['cgroup']['path'] = str(cgroup)
            if wire_report:
                wire_report.write_text(json.dumps(calls, indent=2).replace(str(root), '/srv/m04-fixture') + '\n')
            report.write_text(json.dumps(result, indent=2) + '\n')
            print(json.dumps(result))
        finally:
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--vibe', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--wire-report', type=Path)
    args = parser.parse_args()
    exercise(args.vibe.resolve(), args.report, args.wire_report)
