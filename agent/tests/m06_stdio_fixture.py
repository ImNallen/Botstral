#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys
import time

schema = {'type': 'object', 'properties': {'value': {'type': 'string'}}, 'required': ['value'], 'additionalProperties': False}
mode = sys.argv[1]
for line in sys.stdin:
    message = json.loads(line)
    method = message.get('method')
    if method == 'initialize':
        assert message['params']['protocolVersion'] == '2025-06-18'
        assert message['params']['capabilities'] == {}
        result = {'protocolVersion': '2024-11-05' if mode == 'wrong-version' else '2025-06-18',
                  'capabilities': {'tools': {}}, 'serverInfo': {'name': 'fixture', 'version': '1'}}
    elif method == 'notifications/initialized':
        continue
    elif method == 'tools/list':
        result = {'tools': [{'name': 'echo', 'description': 'Synthetic echo', 'inputSchema': schema}]}
    elif method == 'tools/call':
        assert 'UNRELATED_HOST_SECRET' not in os.environ
        evidence = Path(os.environ['FIXTURE_EVIDENCE'])
        with evidence.open('a') as output:
            output.write(json.dumps({'pid': os.getpid(), 'parent': os.getppid(), 'cgroup': Path('/proc/self/cgroup').read_text() if Path('/proc/self/cgroup').exists() else None}) + '\n')
            output.flush()
            os.fsync(output.fileno())
        if mode == 'lost':
            sys.exit(0)
        if mode == 'deadline':
            time.sleep(10)
        text = os.environ.get('FIXTURE_TOKEN', '') + message['params']['arguments']['value']
        if mode == 'oversized':
            text = 'x' * 140000
        result = {'content': [{'type': 'text', 'text': text}]}
    else:
        raise AssertionError(method)
    print(json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}), flush=True)
