import copy
from dataclasses import FrozenInstanceError, asdict, replace
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import Mock, patch

from first_acceptance_compaction import (CompactionEvidence, FIRST_ACCEPTANCE_CHECKS, HELPER_GATE, combine_acceptance, encoded,
    helper_verdict, probe_compaction, sha, verify_helper)
from first_acceptance_remote import coding_evidence, dispatch
from m05_two_vm import poll

FIXTURES = Path(__file__).with_name('fixtures') / 'first-acceptance'
SOURCE = Path(__file__).resolve().parents[2]


def evidence_fixture():
    wire = json.loads((FIXTURES / 'recorded-wire.json').read_text())
    request = json.loads(wire['requests'][0]['body'])
    response = json.loads(wire['responses'][0]['body'])
    task, execution, call_id = ('00000000-0000-4000-8000-' + str(i).zfill(12) for i in range(1, 4))
    summary = {'id': 'parent-run', 'phase': {'type': 'completed'}, 'accounting': {
        'admitted_turns': 2, 'admitted_coding_calls': 1, 'held_tokens': 0, 'held_cost_microusd': 0,
        'settled_tokens': 60, 'settled_cost_microusd': 30}}
    config = {'revision': 1, 'policy': {'endpoint': 'eu', 'model_id': request['model'], 'context_tokens': 32768,
        'acknowledgment': {'model_developer': 'mistralai', 'developer_terms_url': 'terms',
            'privacy_terms_url': 'privacy', 'release': {'type': 'stable'}},
        'prices': {'input_microusd_per_million_tokens': 200000, 'output_microusd_per_million_tokens': 800000,
                   'billing': 'context_tokens_including_images_at_acknowledged_ceilings'}}}
    view = {'id': call_id, 'task': task, 'phase': 'settled', 'accounting': {'type': 'settled', 'charged_microusd': 10},
            'usage': {'prompt_tokens': 12, 'completion_tokens': 8, 'total_tokens': 20},
            'reservation': {'total_tokens': 33792, 'cost_microusd': 7374}, 'provider_request_id': response['id']}
    prepared = {**request, 'stream': False, 'n': 1, 'service_tier': 'standard_only', 'max_tokens': 1024,
                'parallel_tool_calls': False, 'tool_choice': 'auto'}
    call = {'id': call_id, 'task': task, 'execution': execution, 'generation': 2, 'request': prepared,
        'configuration': config, 'view': view, 'response': response, 'parent': summary, 'parent_configuration': config,
        'lease': {'generation': 2, 'closed': False}, 'task_view': {'state': 'completed', 'approval': {'consumed': True},
            'command': {'execution': execution, 'work': {'spec': {'policy': {'configuration_revision': 1,
                'model_id': request['model']}, 'prompt': request['messages'][-2]['content'], 'limits': {'max_turns': 6, 'max_output_tokens': 1024}}}}}}
    active_call = copy.deepcopy(call)
    active_call['task_view']['state'] = 'running'
    active = {'attempts': [active_call], 'worker_process': {'pid': 42, 'cgroup': 'unit-cgroup'},
        'worker_scope': {'ready': {'scope': {'execution': execution}}, 'pids': [42],
            'proc_cgroups': {'42': 'unit-cgroup'}, 'commands': {'42': '/unit/compaction-runtime/bin/vibe'}}}
    native = {**wire, 'runtime_version': '2.24.2', 'trigger': 'explicit_agent_loop_compact', 'automatic_cli_trigger': False,
        'driver_sha256': sha((FIXTURES / 'compact.py').read_bytes()), 'native_session': 'unit-session',
        'process': active['worker_process'], 'before': request['messages'][:-1], 'started_at_ns': 1, 'finished_at_ns': 2, 'context_tokens_after': 0}
    native['after'] = [*native['before'], {'role': 'user', 'context_boundary': 'compaction', 'injected': True,
        'content': '<compaction_summary>\n' + wire['summary'] + '\n</compaction_summary>'}]
    for phase in ('before', 'after'):
        files = {'session/meta.json': encoded({'session_id': 'unit-session'}),
                 'session/messages.jsonl': ''.join(encoded(m) + '\n' for m in native[phase] if m['role'] != 'system')}
        native[phase + '_files'] = files
        native[phase + '_file_digests'] = {name: sha(text) for name, text in files.items()}
    probe = probe_compaction({'recorded_transport': True, 'offline_passed': True, 'native': native,
        'active_lease': active, 'task_id': task, 'execution_id': execution, 'runtime_content_sha256': 'c' * 64})
    snapshot = {'run_turns': [{'id': 'primary'}], 'coding_calls': [view], 'inferences': []}
    refusals = []
    for name in ('wrong_model', 'unsupported_tool', 'unsupported_capability', 'missing_acknowledgment', 'unconsented_latest', 'unconsented_preview'):
        refusal = {'name': name, 'synthetic': True, 'before': snapshot, 'after': snapshot}
        if name in ('wrong_model', 'unsupported_tool'):
            mutated = copy.deepcopy(request)
            if name == 'wrong_model':
                mutated['model'] = 'other-model'
            else:
                mutated['tools'][0]['function']['name'] = 'unsupported_acceptance_capability'
            refusal.update(request=mutated, status=409, response=encoded({'error': 'invalid_message'}))
        elif name == 'unsupported_capability':
            refusal.update(request={'work': {'type': 'unavailable'}}, status=200, outcome={'code': 'unsupported_execution'})
        else:
            policy = copy.deepcopy(config['policy'])
            if name == 'missing_acknowledgment':
                del policy['acknowledgment']
            else:
                policy['model_id'] = 'mistral-small-preview' if name == 'unconsented_preview' else 'mistral-small-latest'
            refusal.update(request={'policy': policy}, status=400,
                response={'error': 'invalid_json' if name == 'missing_acknowledgment' else 'invalid_input'})
        refusals.append(encoded(refusal))
    refusals = tuple(refusals)
    budget = {'cap_microusd': 1000000, 'maximum_attempts': 8, 'runtime_attempt_bound': 24,
        'reconciled': True, 'reserved_microusd': 58992, 'retained_microusd': 22122, 'run': 'unit-campaign',
        'snapshot_sha256': 'a' * 64, 'ledger': {'cap_microusd': 1000000, 'previous_reserved_microusd': 0,
            'reservations': [{'run': 'unit-campaign', 'microusd': 22122}], 'reconciliations': [{
                'run': 'unit-campaign', 'evidence_sha256': 'a' * 64, 'previous_microusd': 58992,
                'retained_microusd': 22122, 'attempts': [{'kind': 'coding_calls', 'record': call_id, 'retained_microusd': 7374},
                    *[{'kind': 'run_turns', 'record': 'primary-' + str(i), 'retained_microusd': 7374} for i in range(2)]]}]}}
    parent = {'summary': summary, 'request': {'limits': {'max_turns': 8}}, 'turns': [
        {'id': 'primary-' + str(i), 'phase': {'type': 'responded'}, 'accounting': {'type': 'settled', 'charged_microusd': 10},
         'usage': {'prompt_tokens': 12, 'completion_tokens': 8, 'total_tokens': 20}} for i in range(2)]}
    return CompactionEvidence(probe, task, execution, 'unit-session', (call_id,), encoded(native),
        encoded({'attempts': [call]}), encoded([view]), encoded(active), encoded(parent), refusals, encoded(budget),
        encoded({'recorded_transport': False, 'production_binaries': True,
            'binary_sha256': {name: 'b' * 64 for name in ('manager', 'worker', 'native')},
            'manager_process': {'pid': 123, 'unit': 'botstral-m06-unit-manager',
                'executable': '/unit/bin/botstral-manager', 'sha256': 'b' * 64,
                'arguments': ['/unit/bin/botstral-manager', '--state-dir', '/unit/state'],
                'recorded_configuration_present': False}, 'recorded_manager_sha256': None,
            'provider_base': 'https://api.eu.mistral.ai', 'runtime_content_sha256': 'c' * 64,
            'provider_source_sha256': sha((SOURCE / 'manager/src/manager/provider.rs').read_bytes()),
            'relay_source_sha256': sha((SOURCE / 'agent/vibe/runtime.py').read_bytes())}),
        encoded({'errors': [], 'scope': {'pids': [], 'survivors': [], 'ready': {'scope': {'execution': execution}}}, 'secret_scan': {'verdict': 'SCANNED', 'files': 1, 'leaks': []},
                 'temporary_provider_key_removed': True}))


class CompactionEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.evidence = evidence_fixture()

    def altered(self, field, mutate):
        value = json.loads(getattr(self.evidence, field))
        mutate(value)
        return replace(self.evidence, **{field: encoded(value)})

    def test_complete_correlated_evidence_passes(self):
        self.assertEqual(verify_helper(self.evidence)['verdict'], 'VERIFIED')
        with self.assertRaises(FrozenInstanceError):
            self.evidence.task_id = 'changed'

    def test_multiline_provider_summary_matches_saved_json_history(self):
        from first_acceptance_compaction import verify_native
        native = json.loads(self.evidence.native)
        previous = native['summary']
        native['summary'] = 'First retained fact.\nSecond retained fact.'
        native['after'][-1]['content'] = native['after'][-1]['content'].replace(previous, native['summary'])
        response = json.loads(native['responses'][0]['body'])
        response['choices'][0]['message']['content'] = '<summary>\n' + native['summary'] + '\n</summary>'
        native['responses'][0]['body'] = encoded(response)
        native['responses'][0]['sha256'] = sha(native['responses'][0]['body'])
        for path in native['after_files']:
            if path.endswith('/messages.jsonl'):
                native['after_files'][path] = ''.join(encoded(m) + '\n' for m in native['after'] if m['role'] != 'system')
                native['after_file_digests'][path] = sha(native['after_files'][path])
        verify_native(native)

    def test_missing_proof_never_passes(self):
        self.assertEqual(helper_verdict(None)['verdict'], 'NOT VERIFIED')
        for field in asdict(self.evidence):
            with self.subTest(field=field):
                value = asdict(self.evidence)
                del value[field]
                self.assertEqual(helper_verdict(value)['verdict'], 'NOT VERIFIED')

    def test_boundary_without_provider_summary_and_tool_free_primary_fail(self):
        def remove_tools(native):
            request = native['requests'][0]
            request['body'] = encoded({**json.loads(request['body']), 'tools': []})
            request['sha256'] = sha(request['body'])
        mutations = [lambda n: n.update(summary='(no summary available)'),
                     lambda n: n['responses'].clear(),
                     remove_tools,
                     lambda n: n['after_files'].clear(),
                     lambda n: n['source_digests'].update({'core/compaction/manager.py': '0' * 64})]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                with self.assertRaises(ValueError):
                    verify_helper(self.altered('native', mutation))

    def test_matching_boundary_cannot_substitute_a_different_provider_summary(self):
        def forge(native):
            original = native['summary']
            native['summary'] = 'An invented summary.'
            native['after'][-1]['content'] = native['after'][-1]['content'].replace(original, native['summary'])
            for name in native['after_files']:
                native['after_files'][name] = native['after_files'][name].replace(original, native['summary'])
                native['after_file_digests'][name] = sha(native['after_files'][name])
        with self.assertRaisesRegex(ValueError, 'provider response must supply native summary'):
            verify_helper(self.altered('native', forge))

    def test_certificate_and_task_identifiers_without_worker_lease_fail(self):
        for mutation in (lambda a: a['attempts'][0]['lease'].update(closed=True),
                         lambda a: a['attempts'][0]['task_view'].update(state='completed'),
                         lambda a: a['worker_scope'].update(pids=[])):
            with self.assertRaises(ValueError):
                verify_helper(self.altered('active_lease', mutation))

    def test_model_endpoint_response_and_accounting_joins_fail_closed(self):
        mutations = [lambda c: c['attempts'][0].update(execution='other'),
                     lambda c: c['attempts'][0]['request'].update(model='other'),
                     lambda c: c['attempts'][0]['configuration']['policy'].update(endpoint='global'),
                     lambda c: c['attempts'][0]['view'].update(accounting={'type': 'held'}),
                     lambda c: c['attempts'][0]['response'].update(id='unjoined-response'),
                     lambda c: c['attempts'].append(copy.deepcopy(c['attempts'][0])),
                     lambda c: c['attempts'][0]['task_view']['command']['work']['spec']['limits'].update(max_turns=7)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                with self.assertRaises(ValueError):
                    verify_helper(self.altered('coding', mutation))
        with self.assertRaises(ValueError):
            verify_helper(self.altered('parent_run', lambda p: p['summary']['accounting'].update(settled_tokens=40)))

    def test_missing_refusals_and_added_attempts_fail(self):
        with self.assertRaises(ValueError):
            verify_helper(replace(self.evidence, policy_refusals=self.evidence.policy_refusals[:-1]))
        refusal = json.loads(self.evidence.policy_refusals[0])
        refusal['after'] = {'coding_calls': ['unexpected attempt']}
        with self.assertRaises(ValueError):
            verify_helper(replace(self.evidence, policy_refusals=(encoded(refusal), *self.evidence.policy_refusals[1:])))

    def test_failed_policy_control_is_saved_before_its_assertion(self):
        from first_acceptance import Acceptance
        acceptance = Acceptance.__new__(Acceptance)
        acceptance.report = {}
        acceptance.bot, acceptance.token = 'unit-bot', 'unit-token'
        acceptance.addresses = {'listen': 'unit-listener'}
        acceptance.owner = Mock(return_value=json.loads(self.evidence.coding)['attempts'][0]['configuration'])
        acceptance.manager = Mock()
        acceptance.manager.call.side_effect = lambda operation, **values: (
            {'status': 200, 'body': {'unexpected': 'configuration accepted'}} if operation == 'http' else {'coding_calls': []})
        acceptance.save = Mock()
        with self.assertRaises(AssertionError):
            acceptance.helper_policy('task', 'execution', {})
        self.assertEqual(acceptance.report['helper_refusals'][0]['status'], 200)
        self.assertNotIn('acknowledgment', acceptance.report['helper_refusals'][0]['request']['policy'])
        acceptance.save.assert_called_once()

    def test_unreconciled_budget_and_incomplete_cleanup_fail(self):
        for field, mutation in [('budget_evidence', lambda b: b.update(reconciled=False)),
                                ('budget_evidence', lambda b: b.update(runtime_attempt_bound=8)),
                                ('budget_evidence', lambda b: b['ledger'].update(cap_microusd=2000000)),
                                ('cleanup', lambda c: c['secret_scan'].update(verdict='NOT SCANNED'))]:
            with self.assertRaises(ValueError):
                verify_helper(self.altered(field, mutation))

    def test_recorded_transport_cannot_verify_live_acceptance(self):
        recorded = self.altered('provenance', lambda p: p.update(recorded_transport=True, production_binaries=False))
        self.assertEqual(helper_verdict(asdict(recorded))['verdict'], 'NOT VERIFIED')

    def test_relabeling_recorded_transport_cannot_replace_running_manager_proof(self):
        def recorded_process(provenance):
            provenance['manager_process'].update(executable='/unit/bin/manager-fixture',
                arguments=['/unit/bin/manager-fixture', '--ignored', '--exact'], recorded_configuration_present=True)
            provenance['recorded_manager_sha256'] = 'b' * 64
        relabeled = self.altered('provenance', recorded_process)
        self.assertEqual(helper_verdict(asdict(relabeled))['verdict'], 'NOT VERIFIED')
        for name in ('manager', 'worker', 'native'):
            with self.subTest(name=name):
                invalid = self.altered('provenance', lambda p: p['binary_sha256'].update({name: ''}))
                self.assertEqual(helper_verdict(asdict(invalid))['verdict'], 'NOT VERIFIED')

    def test_join_reader_does_not_fall_back_to_run_turns(self):
        import sqlite3
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'state').mkdir()
            database = root / 'state/manager.sqlite3'
            with sqlite3.connect(database) as connection:
                for migration in sorted((SOURCE / 'manager/migrations').glob('*.sql')):
                    connection.executescript(migration.read_text())
            result = coding_evidence(root, self.evidence.task_id)
            self.assertEqual(result['attempts'], [])
            self.assertIn('JOIN coding_leases', result['query'])
            call = json.loads(self.evidence.coding)['attempts'][0]
            with sqlite3.connect(database) as connection:
                connection.execute('INSERT INTO tasks(id,bot,worker,request,view) VALUES(?,?,?,?,?)',
                    (call['task'], 'bot', 'worker', '{}', encoded(call['task_view'])))
                connection.execute('INSERT INTO coding_calls VALUES(?,?,?,?,?,?,?,?)',
                    (call['id'], call['task'], call['execution'], call['generation'], encoded(call['request']),
                     encoded(call['configuration']), encoded(call['view']), encoded(call['response'])))
            self.assertEqual(coding_evidence(root, self.evidence.task_id)['attempts'], [])
            with sqlite3.connect(database) as connection:
                connection.execute('INSERT INTO coding_leases VALUES(?,?,?,0)',
                    (call['task'], call['execution'], call['generation']))
            joined = coding_evidence(root, self.evidence.task_id)['attempts']
            self.assertEqual(len(joined), 1)
            self.assertEqual(joined[0]['request'], call['request'])
            self.assertEqual(joined[0]['response'], call['response'])
            self.assertEqual(joined[0]['lease'], {'generation': 2, 'closed': False})



class CompactionPublicationTests(unittest.TestCase):
    def test_poll_waits_for_empty_and_partially_written_evidence(self):
        expected = {'native_session': 'complete-session', 'summary': 'Complete evidence.'}
        serialized = json.dumps(expected)
        for prefix_bytes in (0, 12):
            with self.subTest(prefix_bytes=prefix_bytes), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                workspace = '00000000-0000-4000-8000-000000000042'
                home = root / 'agent/coding' / workspace
                home.mkdir(parents=True)
                opened, release = threading.Event(), threading.Event()
                failures = []
                def write():
                    try:
                        with (home / 'compaction-evidence.json').open('w') as stream:
                            stream.write(serialized[:prefix_bytes])
                            stream.flush()
                            opened.set()
                            if not release.wait(5):
                                raise TimeoutError('reader did not observe unfinished publication')
                            stream.write(serialized[prefix_bytes:])
                    except Exception as error:
                        failures.append(error)
                writer = threading.Thread(target=write)
                writer.start()
                try:
                    self.assertTrue(opened.wait(5))
                    observed = []
                    def read():
                        value = dispatch(root, {'op': 'compaction_native', 'workspace': workspace})
                        observed.append(value)
                        release.set()
                        return value
                    try:
                        value = poll(read, lambda value: value is not None, 'complete compaction evidence', seconds=5)
                    except json.JSONDecodeError as error:
                        self.fail(f'Poll exposed unfinished evidence: {error}')
                    self.assertEqual(observed[0], None)
                    self.assertEqual(value, expected)
                finally:
                    release.set()
                    writer.join(5)
                self.assertFalse(writer.is_alive())
                self.assertEqual(failures, [])

    def test_permanently_malformed_evidence_never_becomes_ready(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace = '00000000-0000-4000-8000-000000000042'
            home = root / 'agent/coding' / workspace
            home.mkdir(parents=True)
            (home / 'compaction-evidence.json').write_text('{')
            self.assertIsNone(dispatch(root, {'op': 'compaction_native', 'workspace': workspace}))
            with self.assertRaises(TimeoutError):
                poll(lambda: dispatch(root, {'op': 'compaction_native', 'workspace': workspace}),
                     lambda value: value is not None, 'malformed compaction evidence', seconds=.01)


class AggregationTests(unittest.TestCase):
    def test_mismatched_binaries_and_product_changes_block_retention(self):
        evidence = evidence_fixture()
        prior = {'missing': [HELPER_GATE], 'provenance': {'source': 'a' * 40,
            'binary_sha256': json.loads(evidence.provenance)['binary_sha256']}, 'checks': [
                {'name': 'actual_eu_model_requests_and_preview_latest_configuration_blockers', 'verdict': 'VERIFIED',
                 'configuration': json.loads(evidence.coding)['attempts'][0]['configuration']},
                {'name': 'browser', 'verdict': 'VERIFIED', 'task': {'type': 'computer', 'spec': {'runtime': {'sha256': 'browser-runtime'}}}},
                {'name': 'connector', 'verdict': 'VERIFIED', 'task': {'type': 'connector', 'spec': {'runner': {'runtime': 'connector-runtime'}}}}]}
        prior['checks'] += [{'name': name, 'verdict': 'VERIFIED'} for name in FIRST_ACCEPTANCE_CHECKS
                            if name not in {c['name'] for c in prior['checks']}]
        supplement = {'passed': True, 'compaction_evidence': asdict(evidence),
            'binary_sha256': prior['provenance']['binary_sha256'], 'context': {'runtime': 'browser-runtime',
                'runner': {'runtime': 'connector-runtime'}, 'configuration': json.loads(evidence.coding)['attempts'][0]['configuration']}}
        with tempfile.TemporaryDirectory() as directory:
            baseline, helper = Path(directory) / 'baseline.json', Path(directory) / 'helper.json'
            baseline.write_text(encoded(prior))
            original = baseline.read_bytes()
            helper.write_text(encoded(supplement))
            with patch('subprocess.check_output', return_value='manager/tests/new.py\n'):
                result = combine_acceptance(baseline, helper)
                self.assertTrue(result['passed'])
                self.assertEqual(result['retained_checks'][0]['check_sha256'], sha(encoded(prior['checks'][0])))
            with patch('subprocess.check_output', return_value='manager/src/manager/provider.rs\n'):
                with self.assertRaisesRegex(ValueError, 'product changes'):
                    combine_acceptance(baseline, helper)
            changed_config = copy.deepcopy(supplement['context']['configuration'])
            changed_config['policy']['model_id'] = 'mistral-small-2603'
            prior['checks'][0]['configuration'] = changed_config
            supplement['context']['configuration'] = changed_config
            baseline.write_text(encoded(prior))
            helper.write_text(encoded(supplement))
            with patch('subprocess.check_output', return_value='manager/tests/new.py\n'):
                with self.assertRaisesRegex(ValueError, 'durable helper configuration'):
                    combine_acceptance(baseline, helper)
            baseline.write_bytes(original)
            supplement['binary_sha256'] = {'manager': 'changed'}
            helper.write_text(encoded(supplement))
            with self.assertRaisesRegex(ValueError, 'product binaries differ'):
                combine_acceptance(baseline, helper)
            self.assertEqual(baseline.read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
