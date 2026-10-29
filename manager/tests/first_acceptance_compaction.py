import hashlib
import json
from dataclasses import asdict, dataclass
from pathlib import Path
import re

HELPER_GATE = 'configured_helper_model_endpoint_and_privacy_blockers'
RUNTIME_CALL_BOUND = 2 * (3 + 1) * 3
HELPER_ADMISSION_BOUND = 6
CAMPAIGN_ADMISSION_BOUND = HELPER_ADMISSION_BOUND + 2
REFUSALS = frozenset(('wrong_model', 'missing_acknowledgment', 'unconsented_preview', 'unconsented_latest', 'unsupported_tool', 'unsupported_capability'))
FIRST_ACCEPTANCE_CHECKS = frozenset((
    'worker_credentials_cannot_approve_enroll_owner_or_read_credentials',
    'native_desktop_browser_file_exact_approval_and_verified_export',
    'native_desktop_cancellation_confirms_empty_scope',
    'p03_closed_client_live_inference_exact_approval_and_one_effect',
    'manager_restart_preserves_original_routine_and_duplicate_trigger',
    'worker_disconnect_after_effect_reconciles_without_reexecution',
    'native_desktop_snapshot_after_actual_event_retention_gap',
    'denied_altered_expired_consumed_and_revoked_authority_cannot_execute_stale_work',
    'offline_backup_restores_history_memory_credentials_artifacts_and_computer_state',
    'actual_eu_model_requests_and_preview_latest_configuration_blockers'))


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'))


def sha(value):
    return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()


def cost(prices, prompt_tokens, completion_tokens):
    return ((prompt_tokens * prices['input_microusd_per_million_tokens'] + 999999) // 1000000
            + (completion_tokens * prices['output_microusd_per_million_tokens'] + 999999) // 1000000)


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


@dataclass(frozen=True)
class CompactionProbe:
    runtime_version: str
    runtime_digest: str
    source_digests: tuple[tuple[str, str], ...]
    driver_digest: str
    config_digest: str
    trigger: str
    expected_request_digest: str
    recorded_evidence: str


@dataclass(frozen=True)
class CompactionEvidence:
    probe: CompactionProbe
    task_id: str
    execution_id: str
    session_id: str
    coding_call_ids: tuple[str, ...]
    native: str
    coding: str
    owner_calls: str
    active_lease: str
    parent_run: str
    policy_refusals: tuple[str, ...]
    budget_evidence: str
    provenance: str
    cleanup: str


def verify_native(native):
    require(native['runtime_version'] == '2.24.2', 'pinned Vibe required')
    require(native['trigger'] == 'explicit_agent_loop_compact' and native['automatic_cli_trigger'] is False, 'explicit helper identity')
    pinned = json.loads(Path(__file__).with_name('fixtures').joinpath('first-acceptance/pinned.json').read_text())
    require(native['source_digests'] == pinned['sources'], 'pinned compaction source changed')
    before, after = native['before'], native['after']
    require(before and not any(m.get('context_boundary') == 'compaction' for m in before), 'uncorrupted before history')
    require(after[:-1] == before and after[-1]['context_boundary'] == 'compaction', 'new native boundary required')
    summary = native['summary']
    require(summary.strip() and summary != '(no summary available)', 'usable provider summary required')
    require('<compaction_summary>\n' + summary + '\n</compaction_summary>' in after[-1]['content'], 'native summary mismatch')
    require(native['context_tokens_after'] == 0 and native['finished_at_ns'] > native['started_at_ns'], 'compaction lifecycle')
    config = native['configuration']
    require(config['model'] == config['active_model'] and config['provider'] == 'manager'
            and config['alias'] == 'managed-compact' and config['backend'] == 'generic'
            and config['api_style'] == 'openai', 'managed helper configuration')
    require(native['requests'] and len(native['requests']) == len(native['responses']) <= HELPER_ADMISSION_BOUND, 'bounded complete wire evidence')
    for request, response in zip(native['requests'], native['responses']):
        require(sha(request['body']) == request['sha256'] and sha(response['body']) == response['sha256'], 'wire digest mismatch')
        body = json.loads(request['body'])
        require(body['model'] == config['model'] and request['path'] == '/v1/chat/completions', 'generated helper route')
    first = json.loads(native['requests'][0]['body'])
    require(first['messages'][-1]['content'] == config['compaction_prompt'], 'actual pinned compaction prompt required')
    require({t['function']['name'] for t in first['tools']} == {'read_file', 'grep', 'write_file', 'edit'}, 'primary compaction tools required')
    require(sha(config['compaction_prompt']) == pinned['prompts']['compact.md'], 'compaction prompt hash')
    require(sha(config['fallback_prompt']) == pinned['prompts']['compact_system.md'], 'fallback prompt hash')
    successful = [json.loads(r['body']) for r in native['responses'] if r['status'] == 200]
    summaries = [re.search(r'<summary>(.*?)</summary>', r['choices'][0]['message'].get('content') or '', re.S) for r in successful]
    require(any(match and match.group(1).strip() == summary for match in summaries), 'provider response must supply native summary')
    for name in ('before_files', 'after_files'):
        files = native[name]
        require(files and any(path.endswith('meta.json') and json.loads(text)['session_id'] == native['native_session']
                              for path, text in files.items()), 'native session file identity')
        history = before if name == 'before_files' else after
        expected = [{k: v for k, v in m.items() if v is not None} for m in history if m['role'] != 'system']
        saved = [text for path, text in files.items() if path.endswith('/messages.jsonl')]
        require(len(saved) == 1 and [json.loads(line) for line in saved[0].splitlines()] == expected, 'exact saved native messages required')
        digests = native['before_file_digests' if name == 'before_files' else 'after_file_digests']
        require(set(digests) == set(files) and all(re.fullmatch('[0-9a-f]{64}', value) for value in digests.values()), 'native file hashes required')
        require(all(sha(text) == digests[path] for path, text in files.items() if path.endswith('/messages.jsonl')), 'native history file hash')
    require(native['before_files'] != native['after_files'] and any(summary in text for text in native['after_files'].values()), 'saved native history required')


def probe_compaction(record):
    require(record['recorded_transport'] is True and record['offline_passed'] is True, 'successful recorded probe required')
    native = record['native']
    verify_native(native)
    require(native['driver_sha256'] == sha(Path(__file__).with_name('fixtures').joinpath('first-acceptance/compact.py').read_bytes()),
            'recorded fixture changed before live provisioning')
    validate_lease(record['active_lease'], record['task_id'], record['execution_id'])
    require(re.fullmatch('[0-9a-f]{64}', record['runtime_content_sha256']) is not None, 'sealed runtime content identity required')
    return CompactionProbe('2.24.2', record['runtime_content_sha256'], tuple(sorted(native['source_digests'].items())), native['driver_sha256'],
        sha(encoded(native['configuration'])), native['trigger'], native['requests'][0]['sha256'], encoded(record))


def validate_lease(record, task, execution):
    require(record['attempts'], 'durable helper attempts required')
    scope, process = record['worker_scope'], record['worker_process']
    require(scope['ready']['scope']['execution'] == execution and process['pid'] in scope['pids']
            and scope['proc_cgroups'][str(process['pid'])] == process['cgroup']
            and '/compaction-runtime/bin/vibe' in scope['commands'][str(process['pid'])], 'worker launched helper process required')
    for call in record['attempts']:
        require(call['task'] == task and call['execution'] == execution and call['lease']['closed'] is False
                and call['generation'] == call['lease']['generation'] > 0, 'genuine active worker coding lease required')
        view = call['task_view']
        require(view['state'] == 'running' and view['command']['execution'] == execution
                and view['approval']['consumed'] is True, 'active approved worker task required')


def verify_helper(evidence, *, recorded=False):
    probe = probe_compaction(json.loads(evidence.probe.recorded_evidence))
    require(probe == evidence.probe, 'probe integrity')
    native, coding = json.loads(evidence.native), json.loads(evidence.coding)
    verify_native(native)
    require(tuple(sorted(native['source_digests'].items())) == probe.source_digests and native['driver_sha256'] == probe.driver_digest,
            'live helper differs from recorded helper')
    require(native['native_session'] == evidence.session_id, 'native session join')
    active = json.loads(evidence.active_lease)
    validate_lease(active, evidence.task_id, evidence.execution_id)
    require(native['process'] == active['worker_process'], 'native process must own the worker lease')
    calls = coding['attempts']
    require(tuple(c['id'] for c in calls) == evidence.coding_call_ids and len(set(evidence.coding_call_ids)) == len(calls), 'unique coding call joins')
    require([c['view'] for c in calls] == json.loads(evidence.owner_calls), 'owner coding API disagreement')
    require(len(calls) == len(native['requests']) <= HELPER_ADMISSION_BOUND, 'every retry must join a durable attempt')
    for call, request, response in zip(calls, native['requests'], native['responses']):
        body = json.loads(request['body'])
        require(call['task'] == evidence.task_id and call['execution'] == evidence.execution_id and call['view']['id'] == call['id'], 'coding identity mismatch')
        require(call['request']['messages'] == body['messages'] and call['request']['model'] == body['model']
                and call['request'].get('tools', []) == body.get('tools', []), 'exact generated request mismatch')
        require(call['configuration'] == call['parent_configuration'], 'parent configuration mismatch')
        config = call['configuration']
        require(config['policy']['endpoint'] == 'eu' and config['policy']['model_id'] == body['model'], 'configured model and EU endpoint required')
        spec = call['task_view']['command']['work']['spec']
        require(native['before'][-1]['content'] == spec['prompt'], 'worker task must supply native context')
        expected = {'model': body['model'], 'messages': body['messages'], 'stream': False, 'n': 1,
                    'max_tokens': spec['limits']['max_output_tokens'], 'service_tier': 'standard_only'}
        if body.get('tools'):
            expected.update(tools=body['tools'], tool_choice='auto', parallel_tool_calls=False)
        if 'temperature' in body:
            expected['temperature'] = body['temperature']
        require(call['request'] == expected, 'exact manager normalization of generated request')
        require(spec['policy']['configuration_revision'] == config['revision'] and spec['policy']['model_id'] == body['model'], 'configuration revision join')
        require(spec['limits']['max_turns'] == HELPER_ADMISSION_BOUND, 'hard manager admission ceiling required')
        view = call['view']
        require(view['phase'] == 'settled' and view['accounting']['type'] == 'settled'
                and view['usage']['total_tokens'] > 0 and view['provider_request_id'], 'settled usage and released holds required')
        require(view['reservation'] == {'total_tokens': config['policy']['context_tokens'] + spec['limits']['max_output_tokens'],
            'cost_microusd': cost(config['policy']['prices'], config['policy']['context_tokens'], spec['limits']['max_output_tokens'])}, 'full helper reservation required')
        require(response['status'] == 200 and call['response'] == json.loads(response['body']), 'exact durable response mismatch')
        require(call['response']['model'] == body['model'], 'provider response model mismatch')
        require(call['response']['id'] == view['provider_request_id'], 'provider response identity join')
    parent = json.loads(evidence.parent_run)
    accounting = parent['summary']['accounting']
    require(parent['request']['limits']['max_turns'] == CAMPAIGN_ADMISSION_BOUND and parent['summary']['phase']['type'] == 'completed', 'completed bounded parent required')
    require(accounting['admitted_coding_calls'] == len(calls) and accounting['held_tokens'] == accounting['held_cost_microusd'] == 0, 'parent helper accounting mismatch')
    require(all(c['parent']['id'] == parent['summary']['id'] for c in calls), 'parent run join')
    primary = parent['turns']
    require(primary and all(t['phase']['type'] == 'responded' and t['accounting']['type'] == 'settled' for t in primary), 'settled primary attempts required')
    views = [c['view'] for c in calls] + primary
    prices = calls[0]['configuration']['policy']['prices']
    require(all(v['accounting']['charged_microusd'] == cost(prices, v['usage']['prompt_tokens'], v['usage']['completion_tokens'])
                for v in views), 'charged usage must match acknowledged prices')
    require(accounting['admitted_turns'] == len(primary)
            and accounting['settled_tokens'] == sum(v['usage']['total_tokens'] for v in views)
            and accounting['settled_cost_microusd'] == sum(v['accounting']['charged_microusd'] for v in views), 'parent must include every primary and helper charge')
    refusals = [json.loads(r) for r in evidence.policy_refusals]
    require({r['name'] for r in refusals} == REFUSALS and len(refusals) == len(REFUSALS), 'complete policy refusal evidence required')
    for refusal in refusals:
        name, request = refusal['name'], refusal['request']
        expected_status = 409 if refusal['name'] in ('wrong_model', 'unsupported_tool') else 400
        if name == 'unsupported_capability':
            expected_status = 200
            require(request['work']['type'] == 'unavailable' and refusal['outcome']['code'] == 'unsupported_execution', 'actual unsupported capability refusal required')
        elif name in ('wrong_model', 'unsupported_tool'):
            expected = json.loads(native['requests'][0]['body'])
            if name == 'wrong_model':
                require(request['model'] != expected['model'], 'actual model mismatch probe required')
                expected['model'] = request['model']
            else:
                require(request['tools'][0]['function']['name'] == 'unsupported_acceptance_capability', 'actual unsupported tool probe required')
                expected['tools'][0]['function']['name'] = request['tools'][0]['function']['name']
            require(request == expected and json.loads(refusal['response'])['error'] == 'invalid_message', 'captured helper policy probe required')
        else:
            policy = request['policy']
            if name == 'missing_acknowledgment':
                require('acknowledgment' not in policy and refusal['response']['error'] == 'invalid_json', 'actual missing acknowledgment probe required')
            else:
                suffix = '-preview' if name == 'unconsented_preview' else '-latest'
                require(policy['model_id'].endswith(suffix) and policy['acknowledgment']['release']['type'] == 'stable'
                        and refusal['response']['error'] == 'invalid_input', 'actual unconsented model probe required')
        require(refusal['status'] == expected_status and refusal['before'] == refusal['after'] and refusal['synthetic'] is True, 'policy refusal must produce zero attempts')
    budget = json.loads(evidence.budget_evidence)
    require(budget['cap_microusd'] == 1000000 and budget['maximum_attempts'] == CAMPAIGN_ADMISSION_BOUND
            and budget['runtime_attempt_bound'] == RUNTIME_CALL_BOUND and budget['reconciled'] is True, 'finite reconciled campaign required')
    require(0 < budget['reserved_microusd'] <= budget['cap_microusd']
            and budget['retained_microusd'] <= budget['reserved_microusd'], 'campaign exposure')
    require(budget['reserved_microusd'] >= CAMPAIGN_ADMISSION_BOUND * calls[0]['view']['reservation']['cost_microusd'], 'reserve every admitted attempt before provisioning')
    ledger = budget['ledger']
    require(ledger['cap_microusd'] == 1000000 and ledger['previous_reserved_microusd']
            + sum(r['microusd'] for r in ledger['reservations']) <= 1000000, 'campaign cap cannot reset')
    reconciliation = next((r for r in ledger['reconciliations'] if r['run'] == budget['run']
                           and r['evidence_sha256'] == budget['snapshot_sha256']), None)
    require(reconciliation and reconciliation['previous_microusd'] == budget['reserved_microusd']
            and reconciliation['retained_microusd'] == budget['retained_microusd'], 'durable campaign reconciliation')
    require({a['record'] for a in reconciliation['attempts'] if a['kind'] == 'coding_calls'} == set(evidence.coding_call_ids),
            'reconciliation must retain every helper attempt')
    require({a['record'] for a in reconciliation['attempts'] if a['kind'] == 'run_turns'} == {t['id'] for t in primary}
            and sum(a['retained_microusd'] for a in reconciliation['attempts']) == budget['retained_microusd'], 'retain every primary and helper attempt ceiling')
    provenance = json.loads(evidence.provenance)
    require(provenance['runtime_content_sha256'] == probe.runtime_digest, 'live runtime differs from recorded runtime')
    require(provenance['recorded_transport'] is recorded and provenance['production_binaries'] is (not recorded)
            and set(provenance['binary_sha256']) == ({'manager', 'worker'} if recorded else {'manager', 'worker', 'native'}), 'live production provenance required')
    require(all(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value)
                for value in provenance['binary_sha256'].values()), 'valid executable hashes required')
    process = provenance['manager_process']
    executable = Path(process['executable'])
    expected_name = 'manager-fixture' if recorded else 'botstral-manager'
    expected_hash = provenance['recorded_manager_sha256'] if recorded else provenance['binary_sha256']['manager']
    require(process['pid'] > 0 and process['unit'].startswith('botstral-m06-')
            and executable.name == expected_name and process['arguments'][0] == str(executable)
            and process['sha256'] == expected_hash, 'captured running manager must match the executed binary')
    require(process['recorded_configuration_present'] is recorded
            and ('--ignored' in process['arguments']) is recorded
            and (recorded or provenance['recorded_manager_sha256'] is None), 'recorded manager cannot establish live transport')
    require(provenance['provider_base'] == 'https://api.eu.mistral.ai', 'pinned production endpoint selection')
    source = Path(__file__).resolve().parents[2]
    require(provenance['provider_source_sha256'] == sha((source / 'manager/src/manager/provider.rs').read_bytes())
            and provenance['relay_source_sha256'] == sha((source / 'agent/vibe/runtime.py').read_bytes()), 'provider and relay implementation provenance')
    cleanup = json.loads(evidence.cleanup)
    require(cleanup['scope']['pids'] == [] and cleanup['scope']['survivors'] == [], 'empty helper process scope required')
    require(cleanup['scope']['ready']['scope']['execution'] == evidence.execution_id, 'helper cleanup scope identity')
    require(cleanup['errors'] == [] and cleanup['secret_scan']['verdict'] == 'SCANNED'
            and cleanup['secret_scan']['files'] > 0 and cleanup['secret_scan']['leaks'] == []
            and cleanup['temporary_provider_key_removed'] is True, 'complete cleanup and secret scan required')
    return {'verdict': 'RECORDED' if recorded else 'VERIFIED', 'claim': 'Explicit pinned AgentLoop.compact through a production worker coding lease.',
            'automatic_cli_trigger': False, 'evidence_sha256': sha(encoded(asdict(evidence))), 'coding_calls': list(evidence.coding_call_ids)}


def helper_verdict(record):
    if record is None:
        return {'verdict': 'NOT VERIFIED', 'reason': 'No complete helper evidence was supplied.'}
    try:
        return verify_helper(decode_evidence(record))
    except (KeyError, TypeError, ValueError, IndexError, AttributeError, StopIteration) as error:
        return {'verdict': 'NOT VERIFIED', 'reason': str(error)}


def decode_evidence(record):
    value = dict(record)
    value['probe'] = CompactionProbe(**{**value['probe'], 'source_digests': tuple(tuple(p) for p in value['probe']['source_digests'])})
    for name in ('coding_call_ids', 'policy_refusals'):
        value[name] = tuple(value[name])
    return CompactionEvidence(**value)


def policy_identity(configuration):
    policy = configuration['policy']
    acknowledgment = policy['acknowledgment']
    prices = policy['prices']
    return {'endpoint': policy['endpoint'], 'model_id': policy['model_id'], 'context_tokens': policy['context_tokens'],
            'acknowledgment': {k: acknowledgment[k] for k in ('model_developer', 'developer_terms_url', 'privacy_terms_url', 'release')},
            'prices': {k: prices[k] for k in ('input_microusd_per_million_tokens', 'output_microusd_per_million_tokens', 'billing')}}


def retained_runtimes(value):
    found = set()
    def visit(node):
        if isinstance(node, dict):
            if node.get('type') == 'computer' and 'spec' in node:
                found.add(('computer', node['spec']['runtime']['sha256']))
            if node.get('type') == 'connector' and 'spec' in node:
                found.add(('connector', node['spec']['runner']['runtime']))
            for item in node.values():
                visit(item)
        elif isinstance(node, list):
            for item in node:
                visit(item)
    visit(value)
    return found


def source_delta(base):
    import subprocess
    require(re.fullmatch('[0-9a-f]{40}', base) is not None, 'exact baseline source required')
    source = Path(__file__).resolve().parents[2]
    delta = subprocess.check_output(['git', 'diff', '--name-only', base, '--'], cwd=source, text=True).splitlines()
    allowed = ('manager/tests/', 'agent/tests/', 'docs/', 'scripts/')
    require(all(name.startswith(allowed) or name in ('AGENTS.md', 'PLAN.md', 'Makefile', '.gitattributes',
        'manager/src/manager/inference/integrated_fixture.rs', 'manager/src/manager/inference/tests.rs') for name in delta), 'source delta includes product changes')
    return delta


def combine_acceptance(prior_path, supplement_path):
    prior_bytes, supplement_bytes = prior_path.read_bytes(), supplement_path.read_bytes()
    prior, supplement = json.loads(prior_bytes), json.loads(supplement_bytes)
    require(set(prior['missing']) == {HELPER_GATE} and not prior.get('cleanup_errors'), 'baseline must lack only the helper gate')
    verdict = helper_verdict(supplement['compaction_evidence'])
    require(verdict['verdict'] == 'VERIFIED' and supplement['passed'] is True, 'verified live helper supplement required')
    require(prior['provenance']['binary_sha256'] == supplement['binary_sha256'], 'product binaries differ; retained gates require scoped reruns')
    require(json.loads(supplement['compaction_evidence']['provenance'])['binary_sha256'] == supplement['binary_sha256'], 'supplement binary provenance mismatch')
    baseline_config = next(c['configuration'] for c in prior['checks'] if c['name'] == 'actual_eu_model_requests_and_preview_latest_configuration_blockers')
    require(policy_identity(baseline_config) == policy_identity(supplement['context']['configuration']), 'retained configuration differs')
    calls = json.loads(supplement['compaction_evidence']['coding'])['attempts']
    require(all(call['configuration'] == supplement['context']['configuration'] for call in calls), 'durable helper configuration differs from supplement context')
    context = supplement['context']
    require(retained_runtimes(prior) == {('computer', context['runtime']), ('connector', context['runner']['runtime'])}, 'retained runtime identities differ')
    base = prior['provenance'].get('manager_upgrade', {}).get('base', prior['provenance']['source'])
    delta = source_delta(base)
    checks = [c for c in prior['checks'] if c['verdict'] == 'VERIFIED']
    require(FIRST_ACCEPTANCE_CHECKS <= {c['name'] for c in checks}, 'complete immutable retained checks required')
    return {'format': 1, 'passed': True, 'claim': 'Retained first acceptance checks plus explicit Vibe helper integration.',
            'baseline_sha256': sha(prior_bytes), 'supplement_sha256': sha(supplement_bytes),
            'binary_sha256': supplement['binary_sha256'], 'harness_only_delta': delta,
            'retained_checks': [{'name': c['name'], 'source': c.get('source', prior['provenance']['source']),
                                 'compatible_product_source': base, 'source_report_sha256': sha(prior_bytes),
                                 'check_sha256': sha(encoded(c))} for c in checks],
            'helper_routing': verdict, 'automatic_cli_trigger': False}


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--supplement', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = combine_acceptance(args.baseline, args.supplement)
    with args.output.open('x') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')
