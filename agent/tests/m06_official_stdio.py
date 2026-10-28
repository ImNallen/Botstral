#!/usr/bin/env python3
"""Run the installed official filesystem MCP server in an isolated directory."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

spec = importlib.util.spec_from_file_location('connector_runtime', Path(__file__).parents[1] / 'connector/runtime.py')
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


def exercise(node, server, report):
    package = json.loads((server.parent.parent / 'package.json').read_text())
    assert package['name'] == '@modelcontextprotocol/server-filesystem'
    assert package['version'] == '2.0.1'
    with tempfile.TemporaryDirectory(prefix='botstral-m06-official-') as directory:
        root = Path(directory).resolve()
        connection = {'connection': str(uuid.uuid4()), 'revision': 1, 'credential_epoch': 1}
        permit = {'id': str(uuid.uuid4()), 'transport': {'type': 'worker_stdio', 'worker': str(uuid.uuid4()),
                  'account': {'uid': os.getuid(), 'gid': os.getgid()}, 'executable': str(node),
                  'arguments': [str(server), str(root)], 'cwd': str(root), 'environment': {}},
                  'secret': {'type': 'none'}, 'deadline': int(time.time()) + 30, 'max_result_bytes': 4096,
                  'operation': {'type': 'discover', 'connection': connection}}
        discovered = runtime.Stdio(permit).execute()
        assert discovered['type'] == 'finished', discovered
        tools = discovered['result']['tools']
        selected = next(tool for tool in tools if tool['name'] == 'write_file')
        binding = {'connection': connection, 'catalog_revision': 1, 'grant_revision': 1, 'name': selected['name'],
                   'schema_digest': runtime.digest(selected['input_schema'])}
        permit['id'] = str(uuid.uuid4())
        permit['operation'] = {'type': 'call', 'proposal': {'tool': binding,
                              'arguments': {'path': str(root / 'proof.txt'), 'content': 'M06 official connector proof\n'}}}
        result = runtime.Stdio(permit).execute()
        assert result['type'] == 'finished' and not result['result']['value'].get('isError'), result
        assert (root / 'proof.txt').read_text() == 'M06 official connector proof\n'
        proof = {'passed': True, 'server': package['name'], 'version': package['version'],
                 'entry_sha256': hashlib.sha256(server.read_bytes()).hexdigest(),
                 'node': subprocess.check_output([str(node), '--version'], text=True).strip(),
                 'protocol': runtime.VERSION, 'discovered_tools': len(tools), 'tool': 'write_file',
                 'checks': ['real official server', 'exact protocol negotiation', 'discovery', 'schema digest', 'observed temporary file effect'],
                 'external_secrets': False, 'authenticated_remote_acceptance': False}
        report.write_text(json.dumps(proof, indent=2) + '\n')
        print(json.dumps(proof))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--node', type=Path, required=True)
    parser.add_argument('--server', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    exercise(args.node.resolve(), args.server.resolve(), args.report)
