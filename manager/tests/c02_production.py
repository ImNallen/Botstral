#!/usr/bin/env python3
"""Exercise the real native service and retained worker on two isolated VMs."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shlex
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import uuid

import m05_acceptance as m
from m05_two_vm import Remote as LegacyRemote, poll
from c02_reply_proxy import ReplyProxy

SOURCE = 'e81cdbd51e2e852d2e6a78b5505eccf3a43ff8ed'
ROOT = Path(__file__).resolve().parents[2]


def private(path, value):
    with open(path, 'w', opener=lambda p, f: os.open(p, f, 0o600)) as file:
        json.dump(value, file, indent=2)
        file.write('\n')
    path.chmod(0o600)


class Remote(LegacyRemote):
    def install(self):
        bootstrap = "import os,pathlib,json,sys;os.umask(0o077);p=json.load(sys.stdin);r=pathlib.Path(p['root']);r.mkdir(mode=0o700);(r/'bin').mkdir(mode=0o700);[(r/n).write_text(v) for n,v in p['sources'].items()];print('true')"
        self.ssh(['python3', '-c', bootstrap], dict(root=self.root, sources={name: Path(__file__).with_name(name).read_text() for name in ('m05_remote.py', 'c02_production_remote.py')}))

    def call(self, op, **values):
        result = self.ssh(['python3', self.root+'/c02_production_remote.py'], dict(op=op, **values))
        m.require(result.get('ok'), 'remote '+op+' failed on '+self.alias+' '+result.get('error', ''))
        return result['value']


class Native:
    def __init__(self, binary, root):
        with (root/'native.log').open('w') as log:
            self.process = subprocess.Popen([str(binary), str(root)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1)

    def call(self, op, allow_error=False, **values):
        self.process.stdin.write(json.dumps(dict(op=op, **values))+'\n')
        self.process.stdin.flush()
        ready, _, _ = select.select([self.process.stdout], [], [], 150)
        m.require(ready, 'native command timed out '+op)
        line = self.process.stdout.readline()
        m.require(line, 'native driver exited '+str(self.process.poll()))
        value = json.loads(line)
        if not allow_error:
            m.require(value['ok'], 'native '+op+' refused '+str(value['error']))
        return value

    def close(self):
        if self.process.poll() is None:
            self.call('quit')
            self.process.stdin.close()
            m.require(self.process.wait(timeout=15) == 0, 'native driver exit')


def endpoint_refuses(address):
    host, port = address.rsplit(':', 1)
    until = time.monotonic()+10
    while time.monotonic() < until:
        try:
            with socket.create_connection((host, int(port)), timeout=1):
                pass
        except OSError:
            return True
        time.sleep(.1)
    return False


def cleanup(control):
    records = []
    manager = Remote(control['manager_alias'], control['root'])
    manager.files({'stop': b''})
    for item in reversed(control['units']):
        remote = Remote(item['host'], control['root'])
        records.append(dict(host=remote.alias, unit=item['unit'], **remote.call('stop', unit=item['unit'])))
    endpoints = {address: endpoint_refuses(address) for address in control['endpoints']}
    m.require(all(endpoints.values()), 'fixture endpoint still accepts TCP')
    for alias in (control['manager_alias'], control['computer_alias']):
        remote = Remote(alias, control['root'])
        processes = remote.call('root_processes')
        m.require(not processes, 'fixture process remains on '+alias)
        records.append(dict(host=alias, fixture_removed=remote.call('cleanup')))
    return dict(units=records, endpoints_refuse=endpoints)


def invite(args):
    control = json.loads(args.fixture.read_text())
    m.require(re.fullmatch('[0-9a-f]{64}', args.fingerprint), 'public fingerprint format')
    m.require(not args.output.exists() and not args.output.is_symlink(), 'invitation destination exists')
    manager = Remote(control['manager_alias'], control['root'])
    fixture = json.loads(manager.call('read', path='fixture.json'))
    response = manager.call('http', address=fixture['local'], method='POST', path='/v1/sessions', body=dict(password=fixture['password']))
    m.require(response['status'] == 201, 'fixture login')
    response = manager.call('http', address=fixture['local'], method='POST', path='/v1/device-invitations', token=response['body']['token'], body=dict(request=dict(fingerprint=args.fingerprint, label=args.label), lifetime_seconds=600))
    m.require(response['status'] == 200, 'fixture invitation')
    grant = response['body']
    bundle = dict(trust=control['trust'], **{key: grant[key] for key in ('role', 'fingerprint', 'expires_at', 'invitation')})
    descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        json.dump(bundle, file)
        file.flush()
        os.fsync(file.fileno())
    print('PASS invitation saved with mode 0600. Its secret was not printed.')


def run(args):
    os.umask(0o077)
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    identifier = uuid.uuid4().hex[:12]
    root = '/home/botstral/c02-production-'+identifier
    manager, computer = Remote(args.manager, root), Remote(args.computer, root)
    local = Path(tempfile.mkdtemp(prefix='c02-production-private-'))
    build = json.loads(args.build.read_text())
    m.require(build['source_sha'] == SOURCE and build['exit'] == 0, 'frozen-source Linux build required')
    evidence = dict(source_sha=SOURCE, status='BLOCKED', build=build, command=shlex.join([sys.executable, *sys.argv]), method='Production Manager runtime in ignored recorded-provider fixture; macOS RemoteService and OS keychain; unchanged installed worker over actual LAN and relay TCP.', fixture_sources={str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path(__file__), Path(__file__).with_name('c02_reply_proxy.py'), Path(__file__).with_name('c02_production_remote.py'), ROOT/'desktop/src-tauri/examples/c02-remote-driver.rs']}, checks=[], root=root, cleanup=None)
    control = dict(root=root, manager_alias=args.manager, computer_alias=args.computer, units=[], endpoints=[])
    private(args.output/'fixture-control.json', control)
    token = None
    fixture = None
    native = proxy = None
    kept = False

    def passed(name, **details):
        evidence['checks'].append(dict(name=name, status='PASS', **details))
        private(args.output/'results.json', evidence)
        print('PASS '+name, flush=True)

    def start(remote, name, executable, arguments, env=None):
        unit = 'botstral-m05-c02-'+identifier+'-'+name
        remote.call('start', unit=unit, executable=executable, args=arguments, log=name+'.log', env=env or {})
        control['units'].append(dict(host=remote.alias, unit=unit, log=name+'.log'))
        private(args.output/'fixture-control.json', control)
        return unit

    def owner(method, path, body=None, status=200):
        response = manager.call('http', address=fixture['local'], method=method, path=path, body=body or {}, token=token)
        m.require(response['status'] == status, 'fixture owner '+path+' status '+str(response['status']))
        return response['body']

    def snapshot(predicate, label, seconds=90):
        last = poll(lambda: native.call('inspect'), lambda x: x['workspace'] is not None and predicate(x['workspace']), label, seconds)
        return last['workspace']

    def send(prompt, browser=False, allow_error=False):
        return native.call('send', allow_error=allow_error, input=dict(bot=fixture['bot'], session=session['id'], text=prompt, account=dict(uid=1000, gid=1000), runtime=runtime if browser else None, process=True, seconds=120, turns=4, tokens=200000, cost_microusd=200000, output_tokens=1024))

    def task_for(prompt):
        workspace = snapshot(lambda w: any(r['request']['prompt'] == prompt and any(t.get('intent') for t in r['turns']) for r in w['runs']), 'production task proposal')
        run = next(r for r in workspace['runs'] if r['request']['prompt'] == prompt)
        task_id = next(t['intent']['task'] for t in run['turns'] if t.get('intent'))
        return next(t for t in workspace['state']['tasks'] if t['id'] == task_id)

    def approve(task):
        review = native.call('review', id=task['id'])['view']['review']
        m.require(review is not None, 'native exact review')
        return native.call('approve', id=review['id'])

    def completed(task, require_success=True):
        workspace = snapshot(lambda w: any(t['id'] == task['id'] and t['state'] in ('completed', 'failed', 'cancelled') and all(o['state']['type'] in ('available', 'unavailable') for o in t['outputs']) for t in w['state']['tasks']), 'production task completion')
        actual = next(t for t in workspace['state']['tasks'] if t['id'] == task['id'])
        if require_success:
            m.require(actual['state'] == 'completed', 'production task failed '+json.dumps(actual))
        return actual

    try:
        m.certificates(local)
        for remote in (manager, computer):
            remote.install()
            remote.files({'ca.pem': (local/'ca.pem').read_bytes()})
        identities = computer.call('identities')
        runtime = identities['files']['/home/botstral/w01-runtime-76b03bf1/runtime.json']
        passed('retained_worker_and_every_runtime_file_verified', **identities)
        subprocess.run(['ssh', args.computer, shlex.join(['cp', build['artifact']['binary'], root+'/bin/manager-fixture'])], check=True, capture_output=True)
        subprocess.run(['scp', args.computer+':'+root+'/bin/manager-fixture', str(local/'manager-fixture')], check=True, capture_output=True)
        m.require(hashlib.sha256((local/'manager-fixture').read_bytes()).hexdigest() == build['artifact']['sha256'], 'Linux artifact transfer hash')
        manager.binary(local/'manager-fixture', 'manager-fixture')
        manager.binary(args.bore, 'bore')
        bore_hash = hashlib.sha256(args.bore.read_bytes()).hexdigest()
        m.require(bore_hash == '60548b7a145ba334981b19fda7cd0210d24a108a3d0dc113919b33fd2eaa90ab', 'bore v0.6.0 identity')
        manager.files({name: (local/name).read_bytes() for name in ('manager.pem', 'manager.key')})
        computer.files({name: (local/name).read_bytes() for name in ('worker.pem', 'worker.key')})
        page = '''<html><body><input id="value"><button id="save" onclick="document.querySelector('#result').textContent=document.querySelector('#value').value">Save</button><div id="result"></div></body></html>'''
        page_script = "import http.server,os,json\nos.chdir("+repr(root)+")\ns=http.server.ThreadingHTTPServer(('127.0.0.1',0),http.server.SimpleHTTPRequestHandler)\nopen('page-address.json','w').write(json.dumps(s.server_address[1]))\ns.serve_forever()\n"
        computer.files({'page.html': page.encode(), 'bin/page.py': page_script.encode()})
        start(computer, 'page', '/usr/bin/python3', [root+'/bin/page.py'])
        page_port = json.loads(poll(lambda: computer.call('read', path='page-address.json'), lambda x: x is not None, 'page server'))
        manager.files({'config.json': json.dumps(dict(worker_root=root, page='http://127.0.0.1:'+str(page_port)+'/page.html')).encode()})
        manager_unit = start(manager, 'manager', 'bin/manager-fixture', ['--ignored', '--nocapture', 'c02_production_remote_fixture'], dict(BOTSTRAL_C02_FIXTURE_ROOT=root))
        fixture = json.loads(poll(lambda: manager.call('read', path='fixture.json'), lambda x: x is not None, 'Manager fixture', 60))
        token = owner('POST', '/v1/sessions', dict(password=fixture['password']), 201)['token']
        port = fixture['public'].rsplit(':', 1)[1]
        direct = '192.168.1.40:'+port
        control['endpoints'].append(direct)
        trust = owner('POST', '/v1/device-trust', dict(endpoint=direct, server_name='192.168.1.40', ca_pem=(local/'ca.pem').read_text(), certificate_pem=(local/'manager.pem').read_text()))
        evidence['endpoint'] = direct
        evidence['leaf'] = trust['leaf']
        fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert((local/'worker.pem').read_text())).hexdigest()
        grant = owner('POST', '/v1/worker-invitations', dict(fingerprint=fingerprint, lifetime_seconds=600), 201)
        computer.files({'worker-invitation.json': json.dumps(grant).encode()})
        computer.call('worker_enroll', address=direct)
        ceilings = dict(runtime_seconds=120, output_bytes=1024, processes=128, memory_bytes=1073741824)
        process_ceilings = dict(runtime_seconds=120, output_bytes=1024, processes=32, memory_bytes=256*1024*1024)
        computer.files({'agent/computer.json': json.dumps(dict(account=dict(uid=1000, gid=1000), runtime=dict(sha256=runtime), directory='/home/botstral/w01-runtime-76b03bf1', ceilings=ceilings)).encode(), 'agent/process.json': json.dumps(dict(profile='linux_systemd_v1', account=dict(uid=1000, gid=1000), ceilings=process_ceilings)).encode()})
        worker_unit = start(computer, 'worker', 'installed_worker', ['run', '--state-dir', root+'/agent'])
        worker = poll(lambda: owner('GET', '/v1/workers'), lambda x: len(x) == 1 and x[0]['phase'] == 'ready', 'production worker readiness', 90)[0]
        session = owner('POST', '/v1/computer-sessions', dict(bot_id=fixture['bot'], worker_id=worker['id']))
        native = Native(args.native, local)
        request = native.call('begin')['view']['pairing']
        grant = owner('POST', '/v1/device-invitations', dict(request=request, lifetime_seconds=600))
        private(local/'invitation.json', dict(trust=trust, **{key: grant[key] for key in ('role', 'fingerprint', 'expires_at', 'invitation')}))
        candidate = native.call('stage', file=str(local/'invitation.json'))['view']['candidate']
        native.call('confirm', id=candidate['id'])
        send('C02 LAN chat')
        chat = snapshot(lambda w: any(r['request']['prompt'] == 'C02 LAN chat' and r['summary']['phase']['type'] == 'completed' for r in w['runs']), 'LAN durable chat')
        passed('macos_remote_service_real_keychain_actual_lan', endpoint=direct, leaf=trust['leaf'], cursor=chat['cursor'], worker=worker['id'], native_sha256=hashlib.sha256(args.native.read_bytes()).hexdigest())
        start(manager, 'bore', 'bin/bore', ['local', port, '--to', 'bore.pub'])
        log = poll(lambda: manager.call('read', path='bore.log'), lambda x: x and re.search(r'bore\.pub:(\d+)', x), 'public relay', 30)
        relay = 'bore.pub:'+re.search(r'bore\.pub:(\d+)', log).group(1)
        control['endpoints'].append(relay)
        context = ssl.create_default_context(cafile=str(local/'ca.pem'))
        with context.wrap_socket(socket.create_connection(tuple([relay.split(':')[0], int(relay.split(':')[1])]), timeout=10), server_hostname='bore.pub') as tls:
            leaf = hashlib.sha256(tls.getpeercert(binary_form=True)).hexdigest()
        m.require(leaf == trust['leaf'], 'relay changed inner TLS')
        relay_trust = dict(trust, endpoint=relay, server_name='bore.pub')
        private(local/'relay-trust.json', relay_trust)
        candidate = native.call('trust', file=str(local/'relay-trust.json'))['view']['candidate']
        native.call('confirm', id=candidate['id'])
        send('C02 relay chat')
        snapshot(lambda w: any(r['request']['prompt'] == 'C02 relay chat' and r['summary']['phase']['type'] == 'completed' for r in w['runs']), 'relay native chat')
        passed('macos_remote_service_public_relay', endpoint=relay, leaf=leaf)
        send('C02 browser and file work', True)
        browser = task_for('C02 browser and file work')
        m.require(computer.call('read', path='browser.txt') is None, 'file effect before approval')
        approve(browser)
        browser = completed(browser, require_success=False)
        native_browser_passed = browser['state'] == 'completed'
        if not native_browser_passed:
            evidence['checks'].append(dict(name='native_composer_production_browser', status='ISSUES', task=browser))
            private(args.output/'native-browser-failure.json', browser)
            control_request = json.loads(json.dumps(browser['request']))
            control_request['id'] = str(uuid.uuid4())
            control_request['settings']['deadline'] = int(time.time())+120
            spec = control_request['work']['spec']
            spec['deadline'] = int(time.time())+120
            spec['limits']['processes'] = 128
            spec['limits']['memory_bytes'] = 1073741824
            browser = owner('POST', '/v1/tasks', control_request)
            approve(browser)
            browser = completed(browser)
        output = next(o for o in browser['outputs'] if o['declaration']['slot'] == 'text')
        m.require(output['state']['type'] == 'available', 'real browser artifact unavailable')
        native.call('export', task=browser['id'], artifact=output['state']['id'], name='browser-export.txt')
        exported = (local/'browser-export.txt').read_bytes()
        m.require(exported == b'C02 production browser proof', 'real browser DOM result differs')
        m.require(computer.call('read', path='browser.txt') == 'C02 production file proof\n', 'real file effect missing')
        image = next(o for o in browser['outputs'] if o['declaration']['slot'] == 'image')
        native.call('export', task=browser['id'], artifact=image['state']['id'], name='browser.png')
        m.require((local/'browser.png').read_bytes().startswith(b'\x89PNG\r\n\x1a\n'), 'browser screenshot PNG')
        shutil.copy2(local/'browser-export.txt', args.output/'browser-export.txt')
        shutil.copy2(local/'browser.png', args.output/'browser.png')
        passed('production_browser_file_and_native_verified_export_over_relay' if native_browser_passed else 'diagnostic_supported_m04_policy_browser_and_native_export', submission='native composer' if native_browser_passed else 'loopback control with 128 processes and 1 GiB', task=browser['id'], text_sha256=hashlib.sha256(exported).hexdigest(), declaration=output, screenshot_sha256=hashlib.sha256((local/'browser.png').read_bytes()).hexdigest())
        send('C02 cancel process group')
        cancelling = task_for('C02 cancel process group')
        approve(cancelling)
        pids = json.loads(poll(lambda: computer.call('read', path='cancel-pids.json'), lambda x: x is not None, 'production process and child'))
        alive = computer.call('processes', pids=pids)
        m.require(len(alive) == 2, 'real process group was not running')
        native.call('cancel', id=cancelling['id'])
        stopped = snapshot(lambda w: any(t['id'] == cancelling['id'] and t['state'] == 'cancelled' for t in w['state']['tasks']), 'authoritative process stop')
        cancelled = next(t for t in stopped['state']['tasks'] if t['id'] == cancelling['id'])
        m.require(cancelled['outcome']['type'] == 'stopped', 'missing production stop evidence')
        m.require(not computer.call('processes', pids=pids), 'production child survived cancellation')
        passed('installed_worker_process_group_cancellation', task=cancelled['id'], pids=pids, running=alive, outcome=cancelled['outcome'])
        before = native.call('suspend')['workspace']['cursor']
        conversation = str(uuid.uuid4())
        owner('POST', '/v1/conversations', dict(id=conversation, bot=fixture['bot']))
        batch = [dict(address=fixture['local'], method='POST', path='/v1/messages', token=token, body=dict(id=str(uuid.uuid4()), conversation=conversation, text='C02 retained message '+str(i))) for i in range(160)]
        responses = manager.call('http_batch', requests=batch)
        m.require(all(r['status'] == 200 for r in responses), 'retention population')
        replay = owner('GET', '/v1/events/'+str(before)+'/64')
        m.require(replay['type'] == 'snapshot', 'old cursor did not cross retention gap')
        recovered = native.call('resume')['workspace']
        m.require(len(recovered['state']['messages']) == 160 and recovered['cursor'] > before+128, 'retention-gap native recovery')
        passed('native_retention_gap_complete_workspace_recovery', before=before, after=recovered['cursor'], messages=160, replay_type=replay.get('type'))
        restored = native.call('restore')['workspace']
        m.require(len(restored['state']['messages']) == 160, 'native service restart lost retained image')
        passed('native_actor_recreation_restores_keychain_and_workspace', cursor=restored['cursor'])
        send('C02 cancel across manager restart')
        active = task_for('C02 cancel across manager restart')
        approve(active)
        old_pids = pids
        pids = poll(lambda: json.loads(computer.call('read', path='cancel-pids.json')), lambda x: x != old_pids, 'second production process')
        manager.files({'restart': b''})
        fixture = json.loads(poll(lambda: manager.call('read', path='fixture.json'), lambda x: x and json.loads(x)['generation'] == 1, 'manager runtime restart', 45))
        token = owner('POST', '/v1/sessions', dict(password=fixture['password']), 201)['token']
        recovered = snapshot(lambda w: any(t['id'] == active['id'] for t in w['state']['tasks']), 'native restart recovery')
        native.call('refresh')
        native.call('cancel', id=active['id'])
        snapshot(lambda w: any(t['id'] == active['id'] and t['state'] == 'cancelled' for t in w['state']['tasks']), 'restart task cancellation')
        m.require(not computer.call('processes', pids=pids), 'restarted process child survived')
        passed('manager_runtime_restart_during_production_work', generation=fixture['generation'], task=active['id'], pids=pids)
        proxy = ReplyProxy(local, direct)
        private(local/'fault-trust.json', dict(trust, endpoint=proxy.address, server_name='localhost'))
        candidate = native.call('trust', file=str(local/'fault-trust.json'))['view']['candidate']
        native.call('confirm', id=candidate['id'])
        proxy.armed.set()
        lost = send('C02 once lost reply', allow_error=True)
        m.require(not lost['ok'] and lost['error'] == 'unknown_outcome', 'lost reply must remain uncertain '+str(lost['error']))
        snapshot(lambda w: any(r['request']['prompt'] == 'C02 once lost reply' for r in w['runs']), 'lost-reply authoritative reconciliation')
        reconciled = native.call('refresh')
        runs = [r for r in reconciled['workspace']['runs'] if r['request']['prompt'] == 'C02 once lost reply']
        m.require(len(runs) == 1 and not any(i['status'] == 'unknown' for i in reconciled['view']['intents']), 'lost-reply duplicate or unresolved intent')
        once = task_for('C02 once lost reply')
        approve(once)
        completed(once)
        m.require(computer.call('read', path='effects.txt') == 'one effect\n', 'duplicate production effect')
        m.require(proxy.observed == dict(submissions=1, dropped_replies=1), 'fault proxy did not drop exactly one real committed reply')
        passed('native_lost_reply_reconciles_without_replay_or_duplicate_effect', run=runs[0]['request']['id'], task=once['id'], proxy=proxy.observed, effects=1)
        candidate = native.call('trust', file=str(local/'relay-trust.json'))['view']['candidate']
        native.call('confirm', id=candidate['id'])
        proxy.close()
        proxy = None
        native.close()
        native = None
        m.require(not (local/'owner/device.json').exists(), 'native credential file exists')
        evidence['status'] = 'ISSUES' if any(c['status'] == 'ISSUES' for c in evidence['checks']) else 'PASS'
        control['trust'] = relay_trust
        control['runtime'] = runtime
        control['bot'] = fixture['bot']
        control['session'] = session['id']
        control['source_sha'] = SOURCE
        control['profile'] = str(args.output.resolve()/'desktop-profile')
        if args.keep_for_ui:
            private(args.output/'fixture-control.json', control)
            kept = True
            evidence['remaining'] = ['Parent packaged graphical walkthrough and video.', 'Native chooser, sleep, OS notifications and hostile navigation observations.']
            print('PASS isolated fixture retained for parent UI. Control '+str(args.output.resolve()/'fixture-control.json'), flush=True)
    except Exception as error:
        evidence['status'] = 'ISSUES'
        evidence['error'] = str(error)
        raise
    finally:
        if proxy:
            proxy.close()
        if native:
            native.close()
        for item in control['units']:
            try:
                remote = Remote(item['host'], root)
                content = remote.call('read', path=item['log'])
                if content:
                    (args.output/(item['unit']+'.log')).write_text(content)
            except Exception as error:
                evidence.setdefault('log_errors', []).append(type(error).__name__)
        try:
            evidence['retained_after'] = computer.call('identities')
            if not kept:
                evidence['cleanup'] = cleanup(control)
        finally:
            private(args.output/'results.json', evidence)
            shutil.rmtree(local)
    return 0 if evidence['status'] == 'PASS' else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    execute = sub.add_parser('run')
    execute.add_argument('--build', type=Path, required=True)
    execute.add_argument('--bore', type=Path, required=True)
    execute.add_argument('--native', type=Path, default=ROOT/'desktop/src-tauri/target/debug/examples/c02-remote-driver')
    execute.add_argument('--output', type=Path, required=True)
    execute.add_argument('--manager', default='botstral-manager')
    execute.add_argument('--computer', default='botstral-computer')
    execute.add_argument('--keep-for-ui', action='store_true')
    authorize = sub.add_parser('invite')
    authorize.add_argument('--fixture', type=Path, required=True)
    authorize.add_argument('--fingerprint', required=True)
    authorize.add_argument('--label', default='C02 parent graphical acceptance')
    authorize.add_argument('--output', type=Path, required=True)
    stop = sub.add_parser('cleanup')
    stop.add_argument('--fixture', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'run':
        return run(args)
    elif args.command == 'invite':
        invite(args)
    else:
        result = cleanup(json.loads(args.fixture.read_text()))
        private(args.fixture.with_name('cleanup.json'), result)
        print('PASS fixture units, endpoints and remote state removed.')


if __name__ == '__main__':
    raise SystemExit(main())
