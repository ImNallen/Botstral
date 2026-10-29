#!/usr/bin/env python3
import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import shlex
import ssl
import subprocess
import tarfile
import tempfile
import time
import uuid

from m05_two_vm import poll

SOURCE = Path(__file__).resolve().parents[2]
NODE = '/home/botstral/w01-runtime-76b03bf1/node/bin/node'
VIBE = '/home/botstral/m04-vibe-2.24.2'
PASSWORD = 'synthetic M04 login password'
PASSPHRASE = 'synthetic M04 vault passphrase'


def ssh(host, args, **kwargs):
    return subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', host, shlex.join(args)], check=True, **kwargs)


class Remote:
    def __init__(self, host, root):
        self.host, self.root = host, root

    def call(self, op, **body):
        result = ssh(self.host, ['env', 'XDG_RUNTIME_DIR=/run/user/1000', 'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus',
                                'python3', self.root+'/m06_remote.py'], input=json.dumps(dict(op=op, **body)), text=True, capture_output=True, timeout=body.get('timeout', 150)+10)
        value = json.loads(result.stdout)
        assert value['ok'], value
        return value['value']

    def files(self, files):
        return self.call('write', files={k: base64.b64encode(v).decode() for k, v in files.items()})

    def read(self, path):
        return self.call('read', path=path)

    def run(self, args, timeout=120):
        value = self.call('run', args=args, timeout=timeout)
        assert value['code'] == 0, value
        return value['stdout']


def deploy(output):
    identifier = json.loads((output/'deployment.json').read_text())['id'] if (output/'deployment.json').exists() else uuid.uuid4().hex[:12]
    hosts = {'manager': 'botstral-manager', 'agent': 'botstral-computer'}
    root = '/home/botstral/botstral-m06-integrated-'+identifier
    paths = subprocess.check_output(['git', 'ls-files', '-z'], cwd=SOURCE).decode().split('\0')
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode='w:gz') as archive:
        for name in paths:
            if name and (SOURCE/name).is_file():
                archive.add(SOURCE/name, arcname=name, recursive=False)
    archive = data.getvalue()
    (output/'source-files.json').write_text(json.dumps({name: hashlib.sha256((SOURCE/name).read_bytes()).hexdigest() for name in paths if name and (SOURCE/name).is_file()}, indent=2, sort_keys=True)+'\n')
    info = {'id': identifier, 'root': root, 'hosts': hosts, 'base': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=SOURCE, text=True).strip(),
            'source_archive_sha256': hashlib.sha256(archive).hexdigest(), 'binary_sha256': {}}
    for host in hosts.values():
        ssh(host, ['mkdir', '-p', '-m', '700', root], capture_output=True)
        ssh(host, ['mkdir', '-p', '-m', '700', root+'/src', root+'/bin'], capture_output=True)
        ssh(host, ['tar', '-xzf', '-', '-C', root+'/src'], input=archive, capture_output=True)
        remote = Remote(host, root)
        for name in ('m06_remote.py', 'm05_remote.py'):
            ssh(host, ['cp', root+'/src/manager/tests/'+name, root+'/'+name], capture_output=True)
    (output/'deployment.json').write_text(json.dumps(info, indent=2)+'\n')
    worker = Remote(hosts['agent'], root)
    prefix = 'cd '+shlex.quote(root+'/src')+' && PATH=/home/botstral/.cargo/bin:/usr/bin:/bin CARGO_TARGET_DIR='+root+'/target '
    for name, command in [('build-bins', 'cargo build --locked --workspace --bins'),
                          ('build-fixture', 'cargo test --locked -p botstral-manager --lib --no-run --message-format=json')]:
        print('Building '+name, flush=True)
        with (output/(name+'.log')).open('wb') as log:
            result = subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', hosts['agent'], prefix+command], stdout=log, stderr=subprocess.STDOUT, timeout=2400)
        assert result.returncode == 0, name+' failed; read '+str(log.name)
    fixture = None
    for line in (output/'build-fixture.log').read_text().splitlines():
        if line.startswith('{'):
            value = json.loads(line)
            if value.get('reason') == 'compiler-artifact' and value.get('executable') and value['target']['name'] == 'botstral_manager':
                fixture = value['executable']
    assert fixture
    info['fixture_executable'] = fixture
    for source, destination, host in [(fixture, root+'/bin/manager-fixture', hosts['manager']),
                                       (root+'/target/debug/botstral-agent', root+'/bin/botstral-agent', hosts['agent'])]:
        raw = ssh(hosts['agent'], ['cat', source], capture_output=True, timeout=180).stdout
        ssh(host, ['python3', '-c', 'import pathlib,sys;p=pathlib.Path(sys.argv[1]);p.write_bytes(sys.stdin.buffer.read());p.chmod(0o700)', destination], input=raw, capture_output=True, timeout=180)
        info['binary_sha256'][destination.rsplit('/', 1)[1]] = hashlib.sha256(raw).hexdigest()
    node = ssh(hosts['agent'], ['cat', NODE], capture_output=True, timeout=180).stdout
    ssh(hosts['manager'], ['python3', '-c', 'import pathlib,sys;p=pathlib.Path(sys.argv[1]);p.write_bytes(sys.stdin.buffer.read());p.chmod(0o700)', root+'/bin/node'], input=node, capture_output=True)
    info['node_sha256'] = hashlib.sha256(node).hexdigest()
    with tempfile.TemporaryDirectory(prefix='m06-private-dependencies-') as temporary:
        private = Path(temporary).resolve()
        for name in ('package.json', 'package-lock.json', 'server.mjs'):
            (private/name).write_bytes((SOURCE/'manager/tests/fixtures/m06-integrated'/name).read_bytes())
        subprocess.run(['npm', 'ci', '--ignore-scripts', '--no-audit', '--no-fund'], cwd=private, check=True, capture_output=True)
        raw = subprocess.check_output(['tar', '-czf', '-', '-C', str(private), '.'])
        for host in hosts.values():
            ssh(host, ['mkdir', '-p', '-m', '700', root+'/sdk'], capture_output=True)
            ssh(host, ['tar', '-xzf', '-', '-C', root+'/sdk'], input=raw, capture_output=True)
    info['dependency_lock_sha256'] = hashlib.sha256((SOURCE/'manager/tests/fixtures/m06-integrated/package-lock.json').read_bytes()).hexdigest()
    (output/'deployment.json').write_text(json.dumps(info, indent=2)+'\n')
    print('Prepared '+root, flush=True)


def exercise(output):
    import importlib.util
    module = importlib.util.spec_from_file_location('m06_runtime', SOURCE/'agent/connector/runtime.py')
    runtime = importlib.util.module_from_spec(module)
    module.loader.exec_module(runtime)
    info = json.loads((output/'deployment.json').read_text())
    build_root = info['root']
    root = build_root+'/run-'+uuid.uuid4().hex[:8]
    for host in info['hosts'].values():
        ssh(host, ['mkdir', '-m', '700', root], capture_output=True)
        ssh(host, ['cp', '-al', build_root+'/bin', root+'/bin'], capture_output=True)
        for name in ('src', 'sdk'):
            ssh(host, ['ln', '-s', build_root+'/'+name, root+'/'+name], capture_output=True)
        for name in ('m05_remote.py', 'm06_remote.py'):
            ssh(host, ['cp', build_root+'/'+name, root+'/'+name], capture_output=True)
    manager = Remote(info['hosts']['manager'], root)
    worker = Remote(info['hosts']['agent'], root)
    units = []
    report = {'deployment': info, 'fixture_root': root, 'checks': [], 'gates': {name: 'NOT VERIFIED' for name in ('remote', 'restore', 'vibe')}, 'cleanup': []}
    token, addresses = None, None

    def check(name, **facts):
        report['checks'].append({'name': name, **facts})
        (output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
        print('PASS '+name, flush=True)

    def owner(method, path, body=None, status=200):
        result = manager.call('http', address=addresses['local'], method=method, path=path, body=body or {}, token=token)
        assert result['status'] == status, (method, path, result)
        return result['body']

    def start(remote, name, executable, args, environment=None):
        unit = 'botstral-m06-'+info['id']+'-'+name
        remote.call('start', unit=unit, executable=executable, args=args, log=name+'.log', environment=environment or {})
        units.append((remote, unit, name+'.log'))
        return unit

    def stop(remote, unit):
        result = remote.call('stop', unit=unit)
        entry = next(value for value in units if value[1] == unit)
        log = remote.read(entry[2])
        if log:
            (output/entry[2]).write_text(log)
        units.remove(entry)
        report['cleanup'].append({'host': remote.host, 'unit': unit, **result})
        return result

    def manager_start():
        nonlocal addresses, token
        manager.call('unlink', paths=['stop', 'fixture.json'])
        unit = start(manager, 'manager', root+'/bin/manager-fixture', ['manager::inference::tests::integrated_fixture::m06_integrated_remote_fixture', '--ignored', '--exact', '--nocapture'],
                     {'BOTSTRAL_M06_FIXTURE_ROOT': root, 'SSL_CERT_FILE': root+'/ca.pem'})
        value = poll(lambda: manager.read('fixture.json'), lambda v: v is not None, 'production manager fixture', seconds=30)
        addresses = json.loads(value)
        token = owner('POST', '/v1/sessions', {'password': PASSWORD}, status=201)['token']
        return unit

    def manager_stop(unit):
        manager.files({'stop': b''})
        poll(lambda: manager.call('unit', unit=unit), lambda v: v['SubState'] != 'running', 'graceful manager shutdown', seconds=20)
        result = stop(manager, unit)
        assert result['exited']['ExecMainStatus'] == '0', result

    def completed(task, expected='completed'):
        value = poll(lambda: owner('GET', '/v1/tasks/'+task),
                     lambda v: v['state'] in ('completed', 'failed', 'cancelled', 'awaiting_attention'), 'task settlement', seconds=90)
        assert value['state'] == expected, value
        return value

    def approve(task):
        deadline = owner('GET', '/v1/tasks/'+task)['request']['settings']['deadline']
        value = owner('PUT', '/v1/tasks/'+task+'/approval', {'expires_at': min(deadline, int(time.time())+180)})
        binding = value['approval']['binding']
        command = value['command']
        assert binding is not None
        assert all(binding[k] == command[k] for k in ('owner', 'bot', 'task', 'operation', 'execution'))
        assert binding['arguments'] == hashlib.sha256(json.dumps(command['work'], separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()
        return value

    def submit(work, session, status=200):
        task = str(uuid.uuid4())
        deadline = work['spec'].get('deadline', work['spec'].get('limits', {}).get('deadline'))
        value = owner('POST', '/v1/tasks', {'id': task, 'session_id': session, 'conversation_id': None,
            'settings': {'model': 'mistral-small-2506', 'max_turns': 3, 'max_tokens': 200000, 'deadline': deadline, 'max_cost_micros': 200000}, 'work': work}, status=status)
        return value

    def call_work(binding, name, text):
        return {'type': 'connector', 'spec': {'operation': {'type': 'call', 'proposal': {'tool': binding, 'arguments': {'name': name, 'text': text}}},
                'runner': runner, 'deadline': int(time.time())+240,
                'result': {'slot': 'connector', 'name': 'connector.json', 'media_type': 'text_plain', 'max_bytes': 8192}}}

    def discover(connection):
        task = str(uuid.uuid4())
        owner('POST', '/v1/connectors/discover', {'task': task, 'session': session, 'expected': connection['version'], 'runner': runner, 'deadline': int(time.time())+180})
        time.sleep(.3)
        assert owner('GET', '/v1/tasks/'+task)['approval']['binding'] is None
        approve(task)
        completed(task)
        return owner('GET', '/v1/connectors/'+connection['id'])

    def grant(connection, bot, names, revision=0):
        return owner('PUT', '/v1/bots/'+bot+'/connectors', {'connection': connection['version'], 'catalog_revision': connection['catalog_revision'], 'expected_revision': revision, 'tools': names})

    def effects(remote):
        value = remote.read('effects.jsonl')
        return [json.loads(line) for line in value.splitlines()] if value else []

    def artifact(task):
        value = poll(lambda: owner('GET', '/v1/tasks/'+task), lambda v: any(o['state']['type'] == 'available' for o in v['outputs']), 'durable artifact', seconds=20)
        state = next(o['state'] for o in value['outputs'] if o['state']['type'] == 'available')
        return manager.call('artifact', address=addresses['local'], token=token, path='/v1/tasks/'+task+'/artifacts/'+state['id'])

    try:
        for remote in (manager, worker):
            remote.files({name: (SOURCE/'manager/tests'/name).read_bytes() for name in ('m05_remote.py', 'm06_remote.py')})
            facts = remote.run(['/usr/bin/python3', '-c', 'import hashlib,json,platform,pathlib;print(json.dumps({"machine_sha256":hashlib.sha256(pathlib.Path("/etc/machine-id").read_bytes()).hexdigest(),"kernel":platform.release(),"architecture":platform.machine()}))'])
            report[remote.host] = json.loads(facts)
        assert report[manager.host]['machine_sha256'] != report[worker.host]['machine_sha256']
        assert manager.call('hashes', files=['bin/manager-fixture'])['bin/manager-fixture'] == info['binary_sha256']['manager-fixture']
        assert worker.call('hashes', files=['bin/botstral-agent'])['bin/botstral-agent'] == info['binary_sha256']['botstral-agent']
        with tempfile.TemporaryDirectory(prefix='m06-private-tls-') as directory:
            certs = Path(directory)/'tls'
            subprocess.run(['bash', str(SOURCE/'agent/tests/m02_certificates.sh'), str(certs), '192.168.1.40'], check=True, capture_output=True)
            certs /= 'certificates'
            ca = (certs/'ca.pem').read_bytes()
            for remote in (manager, worker):
                remote.files({'ca.pem': ca})
            manager.files({name: (certs/name).read_bytes() for name in ('manager.pem', 'manager.key')})
            worker.files({name: (certs/name).read_bytes() for name in ('worker.pem', 'worker.key')})
            fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert((certs/'worker.pem').read_text())).hexdigest()
        bearer = 'synthetic-m06-'+uuid.uuid4().hex
        manager.files({'bearer': bearer.encode(), 'config.json': b'{"public":"0.0.0.0:0"}', 'recorded.json': b'{}'})
        https_unit = start(manager, 'https', root+'/bin/node', [root+'/sdk/server.mjs', 'https', root])
        https = json.loads(poll(lambda: manager.read('https.json'), lambda v: v is not None, 'real SDK HTTPS listener'))
        assert manager.call('https_auth', port=https['port']) == [401, 401]
        check('real_https_server_rejects_absent_and_wrong_bearer')
        manager_unit = manager_start()
        manager.files({'config.json': json.dumps({'public': addresses['public']}).encode()})
        bot = addresses['bot']
        invitation = owner('POST', '/v1/worker-invitations', {'fingerprint': fingerprint, 'lifetime_seconds': 600}, status=201)
        worker.files({'worker-invitation.json': json.dumps(invitation).encode()})
        worker.call('worker_enroll', address='192.168.1.40:'+addresses['public'].rsplit(':', 1)[1])
        identities = worker.call('runtime', vibe=VIBE)
        runner = identities['runner']
        assert identities['vibe_version'] == '2.24.2'
        report['runtimes'] = identities
        report['rust'] = worker.run(['/home/botstral/.cargo/bin/rustc', '--version']).strip()
        report['node'] = worker.run([NODE, '--version']).strip()
        assert report['node'] == 'v24.22.1'
        worker_unit = start(worker, 'worker', root+'/bin/botstral-agent', ['run', '--state-dir', root+'/agent'])
        current_worker = poll(lambda: owner('GET', '/v1/workers'), lambda v: len(v) == 1 and v[0]['phase'] == 'ready', 'actual worker readiness', seconds=30)[0]
        worker_id = current_worker['id']
        session = owner('POST', '/v1/computer-sessions', {'bot_id': bot, 'worker_id': worker_id})['id']
        poll(lambda: owner('GET', '/v1/computer-sessions/'+session), lambda v: v['status'] == 'confirmed', 'Computer session', seconds=30)
        other_bot = str(uuid.uuid4())
        owner('PUT', '/v1/bots/'+other_bot, {'name': 'No connector grant'})
        owner('PUT', '/v1/bots/'+other_bot+'/permission', {'expected_revision': 1, 'allow': True})
        other_session = owner('POST', '/v1/computer-sessions', {'bot_id': other_bot, 'worker_id': worker_id})['id']
        poll(lambda: owner('GET', '/v1/computer-sessions/'+other_session), lambda v: v['status'] == 'confirmed', 'second bot session', seconds=30)
        connection = owner('POST', '/v1/connectors', {'id': str(uuid.uuid4()), 'label': 'Authenticated SDK filesystem', 'transport': {'type': 'remote_http', 'endpoint': 'https://192.168.1.40:'+str(https['port'])+'/mcp'}})
        connection = owner('PUT', '/v1/connectors/'+connection['id']+'/secret', {'expected_revision': 1, 'secret': {'type': 'bearer', 'value': bearer}})
        connection = discover(connection)
        binding = grant(connection, bot, ['write_receipt'])['tools'][0]
        assert owner('GET', '/v1/bots/'+other_bot+'/connector-tools') == []
        submit(call_work(binding, 'forbidden.txt', 'forbidden'), other_session, status=401)
        value = submit(call_work(binding, 'remote-before.txt', 'Authenticated before restore.'), session)
        remote_task = value['id']
        time.sleep(.5)
        assert not effects(manager), effects(manager)
        approve(remote_task)
        remote_completed = completed(remote_task)
        assert remote_completed['approval']['consumed']
        assert manager.read('remote-before.txt') == 'Authenticated before restore.'
        ledger = owner('GET', '/v1/tasks/'+remote_task+'/connector-calls')
        assert len(ledger) == 1 and ledger[0]['state']['outcome']['type'] == 'finished', ledger
        before_artifact = artifact(remote_task)
        assert before_artifact['status'] == 200
        check('authenticated_remote_production_worker_exact_approval_distinct_grants', task=remote_task, task_view=remote_completed, ledger=ledger, artifact=before_artifact, effect_count=len(effects(manager)))
        conversation = str(uuid.uuid4())
        owner('POST', '/v1/conversations', {'id': conversation, 'bot': bot})
        message = str(uuid.uuid4())
        owner('POST', '/v1/messages', {'id': message, 'conversation': conversation, 'text': 'M06 integrated history survives.'})
        instructions = owner('PUT', '/v1/documents', {'bot': bot, 'kind': 'instructions', 'expected_revision': 0, 'sources': [{'type': 'message', 'id': message}], 'markdown': 'M06 retained instructions.'})
        original_owner = owner('GET', '/v1/owner')
        manager_stop(manager_unit)
        stop(worker, worker_unit)
        count = len(effects(manager))
        backup_script = root+'/src/manager/packaging/backup.py'
        backup = json.loads(manager.run(['python3', backup_script, 'create', '--state-dir', root+'/manager-state', '--destination', root+'/manager-backup']))
        verified = json.loads(manager.run(['python3', backup_script, 'verify', '--backup', root+'/manager-backup']))
        manager.call('rename', source='manager-state', destination='manager-retired')
        restored = json.loads(manager.run(['python3', backup_script, 'restore', '--backup', root+'/manager-backup', '--destination', root+'/manager-state']))
        assert backup['backup_id'] == verified['backup_id'] == restored['backup_id']
        manager_unit = manager_start()
        assert owner('GET', '/v1/status')['vault'] == 'locked'
        assert owner('GET', '/v1/owner') == original_owner
        assert owner('GET', '/v1/tasks/'+remote_task+'/connector-calls') == ledger
        after_artifact = artifact(remote_task)
        assert before_artifact == after_artifact
        assert 'M06 retained instructions.' in json.dumps(owner('POST', '/v1/documents/search/'+bot, {'query': 'retained'}))
        snapshot = owner('GET', '/v1/events/0/64')
        assert any(m['id'] == message and m['text'] == 'M06 integrated history survives.' for m in snapshot['state']['messages']), snapshot
        assert len(effects(manager)) == count
        owner('POST', '/v1/vault/unlock', {'passphrase': PASSPHRASE}, status=204)
        owner('POST', '/v1/inference-configurations/verify', {'bot': bot, 'revision': 1})
        worker_unit = start(worker, 'worker-restored', root+'/bin/botstral-agent', ['run', '--state-dir', root+'/agent'])
        poll(lambda: owner('GET', '/v1/workers'), lambda v: v[0]['phase'] == 'ready', 'retained worker reconnect', seconds=30)
        retained = owner('GET', '/v1/workers')[0]
        assert retained['id'] == worker_id
        value = submit(call_work(binding, 'remote-after.txt', 'Authenticated retained credential.'), session)
        approve(value['id']); completed(value['id'])
        assert manager.read('remote-after.txt') == 'Authenticated retained credential.'
        assert len(effects(manager)) == count+1
        check('offline_backup_restore_locked_unlock_retained_authenticated_credential', backup=backup, owner=original_owner, worker_before=current_worker, worker_after=retained,
              history_message=message, instructions=instructions, artifact_sha256=after_artifact['sha256'], before_effects=count, after_effects=len(effects(manager)))
        report['gates']['restore'] = 'VERIFIED'
        local = owner('POST', '/v1/connectors', {'id': str(uuid.uuid4()), 'label': 'Local SDK filesystem', 'transport': {'type': 'worker_stdio', 'worker': worker_id, 'account': runner['account'],
                    'executable': NODE, 'arguments': [root+'/sdk/server.mjs', 'stdio', root], 'cwd': root, 'environment': {}}})
        local = discover(local)
        local_binding = grant(local, bot, ['write_receipt'])['tools'][0]
        def vibe(name, delay=False):
            workspace = root+'/workspace-'+name.removesuffix('.txt')
            worker.run(['mkdir', '-m', '700', workspace])
            manager.files({'recorded.json': json.dumps({'tool': 'manager_'+runtime.public_name(local_binding), 'arguments': {'name': name, 'text': 'Actual Vibe local effect.', 'delay': delay}}).encode()})
            deadline = int(time.time())+240
            return submit({'type': 'vibe', 'spec': {'policy': {'connectors': {'tools': [local_binding], 'max_calls': 1, 'max_result_bytes': 8192, 'runner': runner},
                'workspace': {'id': str(uuid.uuid4()), 'path': workspace}, 'runtime': identities['vibe'], 'account': runner['account'], 'resources': runner['limits'],
                'configuration_revision': 1, 'model_id': 'mistral-small-2506', 'access': 'edit_files_as_guest_account', 'outputs': []},
                'session': {'type': 'new'}, 'prompt': 'Use the approved connector.', 'limits': {'deadline': deadline, 'max_turns': 3, 'max_total_tokens': 200000, 'max_cost_microusd': 200000,
                    'max_output_tokens': 64, 'max_operations': 1, 'max_concurrent_operations': 1}}}, session)
        value = vibe('vibe-complete.txt')
        approve(value['id']); finished = completed(value['id'])
        assert worker.read('vibe-complete.txt') == 'Actual Vibe local effect.'
        vibe_ledger = owner('GET', '/v1/tasks/'+value['id']+'/connector-calls')
        assert len(vibe_ledger) == 1 and vibe_ledger[0]['state']['outcome']['type'] == 'finished'
        check('actual_pinned_vibe_production_manager_and_local_stdio_complete', task=value['id'], ledger=vibe_ledger, task_view=finished)
        before_journal = worker.call('journal')
        value = vibe('vibe-cancel.txt', True)
        approve(value['id'])
        event = poll(lambda: effects(worker), lambda v: any(e['name'] == 'vibe-cancel.txt' for e in v), 'Vibe local child active', seconds=45)
        event = next(e for e in event if e['name'] == 'vibe-cancel.txt')
        execution = value['command']['execution']
        active_journal = worker.call('journal')
        assert len(active_journal['executions']) == len(before_journal['executions'])+1
        assert [e for e in active_journal['executions'] if e not in before_journal['executions']] == [{'task': value['id'], 'execution': execution, 'work': 'vibe'}]
        policy = worker.call('vibe_policy', workspace=value['request']['work']['spec']['policy']['workspace']['id'])
        connector_name = 'manager_'+runtime.public_name(local_binding)
        assert set(policy['enabled_tools']) == {'read_file', 'grep', 'write_file', 'edit', connector_name}
        assert not policy['profile_bypass'] and not policy['config_bypass']
        assert all(policy['tools'][name]['permission'] == 'ask' for name in ('read_file', 'grep', 'write_file', 'edit'))
        assert policy['tools'][connector_name] == {'permission': 'always'}
        scope = worker.call('scope', execution=execution)
        assert event['pid'] in scope['pids'] and event['parent'] in scope['pids'] and event['descendant'] in scope['pids'], (event, scope)
        expected_cgroup = '0::'+scope['ready']['scope']['cgroup']+'\n'
        assert all(scope['proc_cgroups'][str(pid)] == expected_cgroup for pid in (event['pid'], event['parent'], event['descendant'])), scope
        owner('POST', '/v1/tasks/'+value['id']+'/cancel')
        cancelled = completed(value['id'], 'cancelled')
        empty = poll(lambda: worker.call('scope', execution=execution, observed=scope['pids']), lambda v: not v['pids'] and not v['survivors'], 'cancelled empty scope and no descendants', seconds=20)
        check('one_task_scope_local_child_and_cancellation', task=value['id'], active_scope=scope, stopped_scope=empty, journal_before=before_journal, journal_active=active_journal, actual_vibe_policy=policy, task_view=cancelled)
        value = vibe('vibe-revoke.txt', True)
        approve(value['id'])
        poll(lambda: effects(worker), lambda v: any(e['name'] == 'vibe-revoke.txt' for e in v), 'Vibe call before revocation', seconds=45)
        execution = value['command']['execution']
        revoke_active = worker.call('scope', execution=execution)
        grant(local, bot, [], revision=1)
        stale = worker.call('gateway', task=value['id'], execution=execution, body={'type': 'execute', 'id': str(uuid.uuid4()), 'operation': {'type': 'call', 'proposal': {'tool': local_binding, 'arguments': {'name': 'stale.txt', 'text': 'forbidden'}}}})
        assert stale['status'] == 403, stale
        assert worker.read('stale.txt') is None
        revoke_empty = poll(lambda: worker.call('scope', execution=execution, observed=revoke_active['pids']), lambda v: not v['pids'] and not v['survivors'], 'revoked empty scope', seconds=20)
        revoked_ledger = owner('GET', '/v1/tasks/'+value['id']+'/connector-calls')
        assert len(revoked_ledger) == 1 and revoked_ledger[0]['state']['outcome']['type'] == 'unknown'
        assert revoked_ledger[0]['revoked_during_call_at'] is not None
        check('revoked_live_vibe_rejects_stale_gateway_call', task=value['id'], stale_gateway=stale, ledger=revoked_ledger, active_scope=revoke_active, stopped_scope=revoke_empty)
        report['gates']['vibe'] = 'VERIFIED'
        pending = submit(call_work(binding, 'revoked.txt', 'forbidden'), session)
        owner('POST', '/v1/connectors/'+connection['id']+'/revoke', {'expected_revision': connection['version']['revision']})
        owner('PUT', '/v1/tasks/'+pending['id']+'/approval', {'expires_at': int(time.time())+120}, status=401)
        submit(call_work(binding, 'stale-submit.txt', 'forbidden'), session, status=401)
        assert manager.read('revoked.txt') is None and manager.read('stale-submit.txt') is None
        audit = owner('GET', '/v1/connectors/'+connection['id']+'/audit/0')
        assert any(e['code'] == 'revoked' for e in audit), audit
        check('connection_revocation_rejects_cached_task_approval_and_submission', audit=audit)
        report['gates']['remote'] = 'VERIFIED'
        assert bearer not in json.dumps(report)
        for file in worker.run(['find', root+'/agent/processes', '-name', '*launch.json', '-type', 'f']).splitlines():
            assert bearer not in worker.run(['cat', file])
        check('remote_bearer_absent_from_worker_launch_material')
        manager_stop(manager_unit)
        stop(worker, worker_unit)
        stop(manager, https_unit)
        report['passed'] = True
    except Exception as error:
        report['error'] = type(error).__name__+' '+str(error)
        raise
    finally:
        errors = []
        for remote, unit, log in list(reversed(units)):
            try:
                stop(remote, unit)
            except Exception as error:
                errors.append(type(error).__name__+' '+unit)
        report['cleanup_errors'] = errors
        report['fixtures_retained'] = True
        (output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
        assert not errors, errors

def cleanup(output):
    info = json.loads((output/'deployment.json').read_text())
    report = json.loads((output/'report.json').read_text())
    root = info['root']
    assert root == '/home/botstral/botstral-m06-integrated-'+info['id']
    for host in info['hosts'].values():
        remote = Remote(host, root)
        remote.files({'m06_remote.py': (SOURCE/'manager/tests/m06_remote.py').read_bytes()})
        busy = remote.call('quiescent')
        assert not busy, (host, busy)
        removed = remote.call('cleanup')
        assert removed
        report['cleanup'].append({'host': host, 'root_removed': root, 'selected_root_processes': busy})
    report['fixtures_retained'] = False
    (output/'report.json').write_text(json.dumps(report, indent=2)+'\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--prepare', action='store_true')
    mode.add_argument('--cleanup', action='store_true')
    args = parser.parse_args()
    os.umask(0o077)
    args.output.mkdir(parents=True, exist_ok=True)
    if args.prepare:
        deploy(args.output)
    elif args.cleanup:
        cleanup(args.output)
    else:
        exercise(args.output)


if __name__ == '__main__':
    main()
