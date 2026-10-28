#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
import uuid

module = importlib.util.spec_from_file_location('connector_runtime', Path(__file__).parents[1] / 'connector/runtime.py')
runtime = importlib.util.module_from_spec(module)
module.loader.exec_module(runtime)
SCHEMA = {'type': 'object', 'properties': {'value': {'type': 'string'}}, 'required': ['value'], 'additionalProperties': False}


def binding():
    return {'connection': {'connection': str(uuid.uuid4()), 'revision': 2, 'credential_epoch': 1},
            'catalog_revision': 1, 'grant_revision': 1, 'name': 'echo',
            'schema_digest': runtime.digest(SCHEMA)}


def permit(root, mode='normal'):
    return {'type': 'local', 'id': str(uuid.uuid4()), 'transport': {'type': 'worker_stdio', 'worker': str(uuid.uuid4()),
            'account': {'uid': os.getuid(), 'gid': os.getgid()}, 'executable': sys.executable,
            'arguments': [str(Path(__file__).with_name('m06_stdio_fixture.py').resolve()), mode],
            'cwd': str(root), 'environment': {'FIXTURE_EVIDENCE': str(root / 'effects.jsonl')}},
            'secret': {'type': 'local_environment', 'values': {'FIXTURE_TOKEN': 'synthetic-local-private-token'}},
            'deadline': int(time.time()) + 10, 'max_result_bytes': 8192,
            'operation': {'type': 'call', 'proposal': {'tool': binding(), 'arguments': {'value': 'hello'}}}}


class Contract(unittest.TestCase):
    def test_worker_and_manager_digest_vectors_match(self):
        vectors = json.loads((Path(__file__).parents[2] / 'link/schema/connector-digests.json').read_text())
        for vector in vectors:
            self.assertEqual(runtime.digest(vector['value']), vector['digest'])

    def test_real_stdio_has_clean_environment_redacts_results_and_bounds_failure(self):
        os.environ['UNRELATED_HOST_SECRET'] = 'must-not-inherit'
        for mode, expected in [('normal', 'finished'), ('wrong-version', 'not_sent'), ('lost', 'unknown'), ('oversized', 'unknown'), ('deadline', 'unknown')]:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                launch = permit(root, mode)
                if mode == 'deadline':
                    launch['deadline'] = time.time() + .5
                result = runtime.Stdio(launch).execute()
                self.assertEqual(result['type'], expected)
                self.assertNotIn('synthetic-local-private-token', json.dumps(result))
                effects = (root / 'effects.jsonl').read_text().splitlines() if (root / 'effects.jsonl').exists() else []
                self.assertEqual(len(effects), int(mode != 'wrong-version'))

    def test_recheck_after_discovery_prevents_the_tool_effect(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = runtime.Stdio(permit(root), before_call=lambda: False).execute()
            self.assertEqual(result, {'type': 'not_sent', 'reason': 'authority'})
            self.assertFalse((root / 'effects.jsonl').exists())

    def test_live_revocation_interrupts_stdio_with_an_unknown_effect(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            launch = permit(root, 'deadline')
            result = runtime.Stdio(launch, lambda: not (root / 'effects.jsonl').exists()).execute()
            self.assertEqual(result, {'type': 'unknown', 'reason': 'authority'})

    def test_lost_launch_response_never_launches_on_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            launch = {'directory': str(root), 'task': str(uuid.uuid4()), 'execution': str(uuid.uuid4())}
            p = permit(root)
            events = []
            class Gateway(runtime.Gateway):
                def request(self, body):
                    events.append(body['type'])
                    if body['type'] == 'execute':
                        raise runtime.ConnectorError('transport', True)
                    return {'type': 'status', 'call': {'state': {'type': 'dispatch_possible'}}}
            gateway = Gateway(launch, {})
            with self.assertRaises(runtime.ConnectorError):
                gateway.execute(p['id'], p['operation'])
            with self.assertRaises(runtime.ConnectorError):
                Gateway(launch, {}).execute(p['id'], p['operation'])
            self.assertEqual(events, ['execute', 'status'])
            self.assertFalse((root / 'effects.jsonl').exists())

    def test_lost_settlement_response_resubmits_captured_evidence_only(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            launch = {'directory': str(root), 'task': str(uuid.uuid4()), 'execution': str(uuid.uuid4())}
            p = permit(root)
            events = []
            outcomes = []
            class Gateway(runtime.Gateway):
                def request(self, body):
                    events.append(body['type'])
                    if body['type'] == 'execute':
                        return p
                    if body['type'] == 'status':
                        return {'type': 'status', 'call': {'state': {'type': 'dispatch_possible'}}}
                    outcomes.append(body['outcome'])
                    if len(outcomes) == 1:
                        raise runtime.ConnectorError('transport', True)
                    return {'type': 'status', 'call': {'id': p['id'], 'state': {'type': 'resolved', 'outcome': body['outcome']}}}
            with self.assertRaises(runtime.ConnectorError):
                Gateway(launch, {}).execute(p['id'], p['operation'])
            call = Gateway(launch, {}).execute(p['id'], p['operation'])
            self.assertEqual(call['state']['outcome']['type'], 'finished')
            self.assertEqual(events.count('execute'), 1)
            self.assertEqual(outcomes[0], outcomes[1])
            self.assertEqual(len((root / 'effects.jsonl').read_text().splitlines()), 1)
            self.assertNotIn('synthetic-local-private-token', (root / 'connector-calls' / (p['id'] + '.json')).read_text())


if __name__ == '__main__':
    unittest.main()
