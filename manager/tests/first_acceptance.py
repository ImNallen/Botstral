#!/usr/bin/env python3
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import secrets
import ssl
import subprocess
import time
import uuid
from dataclasses import asdict

from first_acceptance_compaction import (CAMPAIGN_ADMISSION_BOUND, CompactionEvidence,
    FIRST_ACCEPTANCE_CHECKS, HELPER_ADMISSION_BOUND, HELPER_GATE, RUNTIME_CALL_BOUND, encoded, helper_verdict, probe_compaction, sha)

from c02_production import Native, private
from m05_acceptance import WebSocket, certificates
from m05_two_vm import poll
from m06_integrated import Remote as BaseRemote, ssh, SOURCE

PASSWORD = 'synthetic first acceptance password'
PASSPHRASE = 'synthetic first acceptance vault passphrase'
MODEL = 'mistral-small-2603'
ACCOUNT = {'uid': 1000, 'gid': 1000}
PROCESS_LIMITS = {'runtime_seconds': 180, 'output_bytes': 1024, 'processes': 32, 'memory_bytes': 268435456}
COMPUTER_LIMITS = {'runtime_seconds': 120, 'output_bytes': 1024, 'processes': 128, 'memory_bytes': 1073741824}
CONTEXT = 262144
OUTPUT = 1024
CALL_HOLD = (CONTEXT * 200000 + 999999) // 1000000 + (OUTPUT * 800000 + 999999) // 1000000
PLANNED_CALLS = 4


class Remote(BaseRemote):
    def call(self, op, **body):
        result = ssh(self.host, ['env', 'XDG_RUNTIME_DIR=/run/user/1000',
            'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus', 'python3', self.root + '/first_acceptance_remote.py'],
            input=json.dumps(dict(op=op, **body)), text=True, capture_output=True, timeout=body.get('timeout', 150) + 10)
        value = json.loads(result.stdout)
        assert value['ok'], (self.host, op, value.get('error'))
        return value['value']


class OwnerSocket(WebSocket):
    def call(self, operation):
        for attempt in range(5):
            request = str(uuid.uuid4())
            self.send({'type': 'call', 'id': request, 'operation': operation})
            start = self.receive()
            while start['type'] in ('pong', 'notification'):
                start = self.receive()
            if start['type'] == 'error':
                assert start['id'] == request, start
                if start['code']['type'] == 'busy' and attempt < 4:
                    time.sleep(.05)
                    continue
                return start
            assert start['type'] == 'transfer_start' and start['request'] == request, start
            content = bytearray()
            for index in range(start['chunks']):
                chunk = self.receive()
                while chunk['type'] in ('pong', 'notification'):
                    chunk = self.receive()
                assert chunk['type'] == 'transfer_chunk' and chunk['id'] == start['id'] and chunk['index'] == index
                content.extend(base64.b64decode(chunk['data'], validate=True))
            complete = self.receive()
            while complete['type'] in ('pong', 'notification'):
                complete = self.receive()
            assert complete == {'type': 'transfer_complete', 'id': start['id']}
            assert len(content) == start['bytes'] and hashlib.sha256(content).hexdigest() == start['digest']
            return json.loads(content)


class Acceptance:
    def __init__(self, args):
        self.args = args
        self.model = 'mistral-small-2506' if args.recorded else MODEL
        self.output = args.output.resolve()
        self.output.mkdir(mode=0o700, parents=True, exist_ok=False)
        self.build = json.loads(args.build.read_text())
        self.identifier = uuid.uuid4().hex[:12]
        self.run_id = str(uuid.uuid4())
        self.root = '/home/botstral/first-acceptance-fixtures/' + self.run_id
        self.workspace = self.root + '/workspace'
        self.manager = Remote(self.build['hosts']['manager'], self.root)
        self.worker = Remote(self.build['hosts']['agent'], self.root)
        self.units = []
        self.addresses = None
        self.token = None
        self.native = None
        self.socket = None
        harness = [Path(__file__), Path(__file__).with_name('first_acceptance_remote.py'),
            SOURCE / 'manager/tests/fixtures/m06-integrated/acceptance-server.mjs']
        if args.suite == 'helper':
            harness += [Path(__file__).with_name('first_acceptance_compaction.py'),
                SOURCE / 'manager/tests/fixtures/first-acceptance/compact.py',
                SOURCE / 'manager/tests/fixtures/first-acceptance/pinned.json']
        self.report = {'id': self.identifier, 'root': self.root, 'workspace': self.workspace, 'source': self.build['base'],
            'production_binaries': True, 'provider': {'endpoint': 'eu', 'model': MODEL},
            'checks': [], 'cleanup': [], 'passed': False,
            'harness_sha256': {str(path.relative_to(SOURCE)): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in harness}}
        for path in harness:
            (self.output / (path.name + '.source')).write_bytes(path.read_bytes())

    def save(self):
        self.report['context'] = {name: getattr(self, name) for name in ('bot', 'permission', 'worker_id', 'session',
            'runtime', 'runner', 'configuration', 'connection', 'binding', 'trust', 'addresses', 'ledger',
            'browser_task', 'browser_artifact', 'browser_run', 'history_conversation', 'history_messages') if hasattr(self, name)}
        private(self.output / 'results.json', self.report)

    def check(self, name, **evidence):
        self.report['checks'].append({'name': name, 'verdict': 'VERIFIED', **evidence})
        self.save()
        print('PASS ' + name, flush=True)

    def write_evidence(self):
        upgrade = self.report.get('manager_upgrade')
        scan = self.report.get('provider_secret_scan')
        if scan and scan.get('not_provisioned'):
            scan = {'files': 0, 'leaks': None, 'verdict': 'NOT SCANNED', 'reason': 'comparison key unavailable'}
        provenance = {name: self.report.get(name) for name in ('source', 'binary_sha256',
            'build_source_manifest_sha256', 'harness_sha256')}
        provenance['source_report_sha256'] = hashlib.sha256((self.output / 'results.json').read_bytes()).hexdigest()
        provenance['evidence_writer_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        if upgrade:
            provenance['manager_upgrade'] = {name: upgrade[name] for name in ('base', 'root',
                'source_archive_sha256', 'changed_paths', 'compiler_artifact', 'manager_sha256')}
        private(self.output / 'evidence.json', {'format': 1, 'passed': self.report['passed'],
            'missing': self.report['missing'], 'provenance': provenance, 'checks': self.report['checks'],
            'helper_routing': self.report['helper_routing'], 'cleanup_errors': self.report.get('cleanup_errors', []),
            'compaction_evidence': self.report.get('compaction_evidence'), 'error': self.report.get('error'),
            'new_provider_attempts': self.report.get('continuation_budget', {}).get('new_attempt_ids', []),
            'provider_secret_scan': scan})

    def owner(self, method, path, body=None, status=200):
        response = self.manager.call('http', address=self.addresses['listen'], method=method,
            path=path, body=body or {}, token=self.token)
        assert response['status'] == status, (method, path, response)
        return response['body']

    def start(self, remote, name, executable, args, environment=None):
        unit = 'botstral-m06-first-' + self.identifier + '-' + name
        remote.call('start', unit=unit, executable=executable, args=args, log=name + '.log', environment=environment or {})
        self.units.append((remote, unit, name + '.log'))
        self.report['units'] = [{'host': r.host, 'unit': u, 'log': log} for r, u, log in self.units]
        self.save()
        return unit

    def stop(self, remote, unit):
        result = remote.call('stop', unit=unit)
        entry = next(item for item in self.units if item[1] == unit)
        content = remote.read(entry[2])
        if content is not None:
            (self.output / entry[2]).write_text(content)
        self.units.remove(entry)
        self.report['units'] = [{'host': r.host, 'unit': u, 'log': log} for r, u, log in self.units]
        self.report['cleanup'].append({'host': remote.host, 'unit': unit, **result})
        self.save()
        return result

    def start_manager(self, name):
        if self.args.recorded:
            build = self.worker.run(['/usr/bin/env', 'PATH=/home/botstral/.cargo/bin:/usr/bin:/bin',
                'RUSTUP_TOOLCHAIN=1.93.0', 'CARGO_TARGET_DIR=' + self.build['root'] + '/target',
                '/home/botstral/.cargo/bin/cargo', 'test', '--manifest-path', self.build['root'] + '/src/Cargo.toml',
                '--locked', '-p', 'botstral-manager', '--lib', '--no-run', '--message-format=json'], timeout=240)
            (self.output / 'recorded-manager-build.jsonl').write_text(build)
            artifacts = [json.loads(line) for line in build.splitlines() if line.startswith('{')]
            artifact = next(value for value in artifacts if value.get('reason') == 'compiler-artifact'
                and value.get('executable') and value['target']['name'] == 'botstral_manager')
            raw = ssh(self.worker.host, ['cat', artifact['executable']], capture_output=True).stdout
            self.manager.call('unlink', paths=['bin/manager-fixture'])
            self.manager.files({'bin/manager-fixture': raw, 'config.json': b'{"public":"0.0.0.0:0"}',
                'recorded.json': json.dumps({'compaction_summary': 'The recorded helper retained the acceptance fixture context.',
                    'compaction_mode': self.args.recorded_mode}).encode()})
            self.report['recorded_manager_sha256'] = sha(raw)
            self.report['production_binaries'] = False
            self.manager_unit = self.start(self.manager, name, self.root + '/bin/manager-fixture',
                ['manager::inference::tests::integrated_fixture::m06_integrated_remote_fixture', '--ignored', '--exact', '--nocapture'],
                {'BOTSTRAL_M06_FIXTURE_ROOT': self.root})
            addresses = json.loads(poll(lambda: self.manager.read('fixture.json'), lambda v: v is not None, 'recorded manager listeners'))
            self.addresses = {'listen': addresses['local'], 'setup': addresses['local'], 'public': addresses['public']}
            self.manager.run(['ln', '-s', 'manager-state', self.root + '/state'])
            return
        self.manager.call('unlink', paths=['state/listeners.json'])
        self.manager_unit = self.start(self.manager, name, self.root + '/bin/botstral-manager', [
            '--state-dir', self.root + '/state', '--listen', self.addresses['listen'] if self.addresses else '127.0.0.1:0',
            '--setup-listen', '127.0.0.1:0', '--public-listen', self.addresses['public'] if self.addresses else '0.0.0.0:0',
            '--public-tls-cert', self.root + '/manager.pem', '--public-tls-key', self.root + '/manager.key',
            '--public-worker-ca', self.root + '/ca.pem'], {'SSL_CERT_FILE': self.root + '/ca.pem'})
        self.addresses = json.loads(poll(lambda: self.manager.read('state/listeners.json'),
            lambda value: value is not None, 'production manager listeners', seconds=30))

    def login(self):
        self.token = self.owner('POST', '/v1/sessions', {'password': 'synthetic M04 login password' if self.args.recorded else PASSWORD}, status=201)['token']

    def unlock(self):
        self.owner('POST', '/v1/vault/unlock', {'passphrase': 'synthetic M04 vault passphrase' if self.args.recorded else PASSPHRASE}, status=204)

    def open_socket(self, first=False):
        assert self.socket is None
        self.socket = OwnerSocket(self.direct, self.context, hostname='192.168.1.40')
        message = {'type': 'enroll' if first else 'authenticate', 'version': 3,
            'manager': self.trust['manager'], 'role': 'owner', 'credential': self.credential}
        if first:
            message['invitation'] = self.invitation['invitation']
        self.socket.send(message)
        assert self.socket.receive()['type'] == 'welcome'

    def call(self, operation):
        assert self.socket is not None
        response = self.socket.call(operation)
        assert response['type'] != 'error', response
        return response['value']

    def close_clients(self):
        if self.socket:
            self.socket.close()
            self.socket = None
        if self.native:
            self.native.call('suspend')
        self.report['closed_client_at'] = time.time()
        self.save()

    def routine(self, action):
        return self.call({'method': 'routine_command', 'input': {'id': str(uuid.uuid4()), 'action': action}})

    def effects(self):
        text = self.manager.read('effects.jsonl')
        return [json.loads(line) for line in text.splitlines()] if text else []

    def stable_effects(self, expected, seconds=3):
        until = time.monotonic() + seconds
        while True:
            assert self.effects() == expected, 'external effect repeated'
            if time.monotonic() >= until:
                return
            time.sleep(.2)

    def observe(self):
        return self.manager.call('observe')

    def task(self, identity, terminal=True):
        return poll(lambda: self.owner('GET', '/v1/tasks/' + identity),
            lambda value: value['state'] in ('completed', 'failed', 'cancelled', 'awaiting_attention') if terminal else value['state'] == 'running',
            'task ' + identity, seconds=150)

    def approve(self, identity):
        value = self.owner('GET', '/v1/tasks/' + identity)
        return self.owner('PUT', '/v1/tasks/' + identity + '/approval',
            {'expires_at': min(value['request']['settings']['deadline'], int(time.time()) + 180)})

    def prepare(self):
        build_root = self.build['root']
        manifest = json.loads(self.args.build.with_name('source-files.json').read_text())
        assert all(hashlib.sha256((SOURCE / name).read_bytes()).hexdigest() == digest for name, digest in manifest.items())
        self.report['build_source_manifest_sha256'] = hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest()
        compiled = {name: digest for name, digest in manifest.items() if name.startswith(('manager/src/', 'agent/src/', 'link/', 'desktop/'))
            and (name.endswith(('.rs', '.ts', '.tsx', '.toml', '.json')) or name.endswith('/build.rs'))}
        assert compiled and all(hashlib.sha256((SOURCE / name).read_bytes()).hexdigest() == digest for name, digest in compiled.items())
        if self.args.native:
            self.build_native(manifest)
        self.prepare_guests(manifest, compiled, build_root)

    def build_native(self, manifest):
        with (self.output / 'native-current-build.log').open('wb') as log:
            subprocess.run(['cargo', 'build', '--manifest-path', str(SOURCE / 'desktop/src-tauri/Cargo.toml'),
                '--no-default-features', '--locked', '--example', 'c02-remote-driver', '--message-format=json'],
                check=True, stdout=log, stderr=subprocess.STDOUT)
        artifacts = [json.loads(line) for line in (self.output / 'native-current-build.log').read_text().splitlines() if line.startswith('{')]
        native_artifact = next(value for value in artifacts if value.get('reason') == 'compiler-artifact'
            and value['target']['name'] == 'c02-remote-driver' and value.get('executable'))
        assert Path(native_artifact['executable']).resolve() == self.args.native.resolve()
        assert all(hashlib.sha256((SOURCE / name).read_bytes()).hexdigest() == digest for name, digest in manifest.items())
        self.report['native_compiler_artifact'] = native_artifact

    def prepare_guests(self, manifest, compiled, build_root):
        self.report['compiled_source_manifest'] = compiled
        for remote in (self.manager, self.worker):
            ssh(remote.host, ['mkdir', '-p', '-m', '700', str(Path(self.root).parent)], capture_output=True)
            ssh(remote.host, ['mkdir', '-m', '700', self.root], capture_output=True)
            ssh(remote.host, ['cp', '-al', build_root + '/bin', self.root + '/bin'], capture_output=True)
            ssh(remote.host, ['ln', '-s', build_root + '/src', self.root + '/src'], capture_output=True)
            ssh(remote.host, ['cp', '-al', build_root + '/sdk', self.root + '/sdk'], capture_output=True)
            for name in ('m05_remote.py', 'm06_remote.py', 'first_acceptance_remote.py'):
                raw = (SOURCE / 'manager/tests' / name).read_bytes()
                ssh(remote.host, ['python3', '-c',
                    'import pathlib,sys;p=pathlib.Path(sys.argv[1]);p.write_bytes(sys.stdin.buffer.read());p.chmod(0o600)',
                    self.root + '/' + name], input=raw, capture_output=True)
        source_checks = [remote.call('verify_sources', manifest=manifest) for remote in (self.manager, self.worker)]
        assert all(not value['changed'] for value in source_checks)
        self.report['remote_source_checks'] = source_checks
        build = self.worker.run(['/usr/bin/env', 'PATH=/home/botstral/.cargo/bin:/usr/bin:/bin', 'RUSTUP_TOOLCHAIN=1.93.0',
            'CARGO_TARGET_DIR=' + build_root + '/target', '/home/botstral/.cargo/bin/cargo', 'build',
            '--manifest-path', build_root + '/src/Cargo.toml', '--locked', '--release', '-p', 'botstral-manager', '-p', 'botstral-agent',
            '--bins', '--message-format=json'], timeout=180)
        (self.output / 'linux-current-build.jsonl').write_text(build)
        built = [json.loads(line) for line in build.splitlines() if line.startswith('{')]
        self.report['linux_compiler_artifacts'] = [value for value in built if value.get('reason') == 'compiler-artifact'
            and value['target']['name'] in ('botstral-manager', 'botstral-agent') and value.get('executable')]
        assert {value['target']['name'] for value in self.report['linux_compiler_artifacts']} == {'botstral-manager', 'botstral-agent'}
        assert all(value['executable'] == build_root + '/target/release/' + value['target']['name']
            and not value['profile']['test'] and 'nonproduction-test-worker' not in value['features']
            for value in self.report['linux_compiler_artifacts'])
        binary = ssh(self.worker.host, ['cat', build_root + '/target/release/botstral-manager'], capture_output=True).stdout
        self.manager.call('unlink', paths=['bin/botstral-manager'])
        self.manager.files({'bin/botstral-manager': binary})
        expected = hashlib.sha256(binary).hexdigest()
        assert self.manager.call('hashes', files=['bin/botstral-manager'])['bin/botstral-manager'] == expected
        worker_binary = ssh(self.worker.host, ['cat', build_root + '/target/release/botstral-agent'], capture_output=True).stdout
        self.worker.call('unlink', paths=['bin/botstral-agent'])
        self.worker.files({'bin/botstral-agent': worker_binary})
        worker_digest = hashlib.sha256(worker_binary).hexdigest()
        assert self.worker.call('hashes', files=['bin/botstral-agent'])['bin/botstral-agent'] == worker_digest
        self.report['binary_sha256'] = {'manager': expected, 'worker': worker_digest,
            **({'native': hashlib.sha256(self.args.native.read_bytes()).hexdigest()} if self.args.native else {})}
        certificates(self.output)
        self.context = ssl.create_default_context(cafile=str(self.output / 'ca.pem'))
        for remote in (self.manager, self.worker):
            remote.files({'ca.pem': (self.output / 'ca.pem').read_bytes()})
        self.manager.files({name: (self.output / name).read_bytes() for name in ('manager.pem', 'manager.key')})
        self.worker.files({'certs/' + name: (self.output / name).read_bytes() for name in ('ca.pem', 'worker.pem', 'worker.key')})
        identities = [json.loads(remote.run(['/usr/bin/python3', '-c',
            'import pathlib,hashlib,json;print(json.dumps({"machine":hashlib.sha256(pathlib.Path("/etc/machine-id").read_bytes()).hexdigest()}))']))
            for remote in (self.manager, self.worker)]
        assert identities[0] != identities[1]
        self.check('same_source_production_binaries_on_distinct_linux_guests', identities=identities, binary_sha256=self.report['binary_sha256'])
        self.start_manager('manager')
        if not self.args.recorded:
            proof = self.manager.read('state/bootstrap.token')
            response = self.manager.call('http', address=self.addresses['setup'], method='POST', path='/v1/owner', token=proof,
                body={'name': 'First acceptance owner', 'password': PASSWORD, 'vault_passphrase': PASSPHRASE, 'acknowledge_no_escrow': True})
            assert response['status'] == 201
        self.login()
        if not self.args.recorded:
            self.unlock()
        self.bot = str(uuid.uuid4())
        self.owner('PUT', '/v1/bots/' + self.bot, {'name': 'First acceptance bot'})
        self.permission = self.owner('PUT', '/v1/bots/' + self.bot + '/permission', {'expected_revision': 1, 'allow': True})
        self.direct = '192.168.1.40:' + self.addresses['public'].rsplit(':', 1)[1]
        self.trust = self.owner('POST', '/v1/device-trust', {'endpoint': self.direct, 'server_name': '192.168.1.40',
            'ca_pem': (self.output / 'ca.pem').read_text(), 'certificate_pem': (self.output / 'manager.pem').read_text()})
        fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert((self.output / 'worker.pem').read_text())).hexdigest()
        invitation = self.owner('POST', '/v1/worker-invitations', {'fingerprint': fingerprint, 'lifetime_seconds': 600}, status=201)
        self.worker.files({'worker-invitation.json': json.dumps(invitation).encode()})
        self.worker.run([self.root + '/bin/botstral-agent', 'enroll', '--state-dir', self.root + '/agent', '--manager', 'wss://' + self.direct,
            '--manager-ca', self.root + '/certs/ca.pem', '--client-cert', self.root + '/certs/worker.pem',
            '--client-key', self.root + '/certs/worker.key', '--invitation-file', self.root + '/worker-invitation.json'])
        raw = ssh(self.worker.host, ['cat', '/usr/bin/python3'], capture_output=True).stdout
        self.worker.files({'tools/python3': raw})
        ssh(self.worker.host, ['mkdir', '-m', '700', self.workspace], capture_output=True)
        self.worker.files({'guest-workspace.json': json.dumps(self.workspace).encode()})
        self.worker.run(['chmod', '700', self.root + '/tools/python3'])
        digest = hashlib.sha256(raw).hexdigest()
        helper = (SOURCE / 'agent/connector/runtime.py').read_bytes()
        self.runner = {'runtime': hashlib.sha256(helper + digest.encode()).hexdigest(), 'account': ACCOUNT, 'limits': PROCESS_LIMITS}
        self.worker.run(['cp', '-rL', '/home/botstral/w01-runtime-76b03bf1', self.root + '/browser-runtime'])
        self.worker.run(['chmod', '700', self.root + '/browser-runtime'])
        self.runtime = hashlib.sha256(ssh(self.worker.host, ['cat', self.root + '/browser-runtime/runtime.json'], capture_output=True).stdout).hexdigest()
        self.worker.files({'agent/process.json': json.dumps({'profile': 'linux_systemd_v1', 'account': ACCOUNT, 'ceilings': PROCESS_LIMITS}).encode(),
            'agent/connector.json': json.dumps({'python': self.root + '/tools/python3', 'python_sha256': digest,
                'runtime': self.runner['runtime'], 'account': ACCOUNT, 'ceilings': PROCESS_LIMITS}).encode(),
            'agent/computer.json': json.dumps({'account': ACCOUNT, 'runtime': {'sha256': self.runtime},
                'directory': self.root + '/browser-runtime', 'ceilings': COMPUTER_LIMITS}).encode()})
        if self.args.suite == 'helper':
            self.prepare_compaction()
        diagnostics = self.worker.call('run', args=[self.root + '/bin/botstral-agent', 'check', '--state-dir', self.root + '/agent'], timeout=120)
        assert diagnostics['code'] == 0 and diagnostics['stdout'].strip() == 'worker ready'
        assert 'computer capability unavailable' not in diagnostics['stderr'], diagnostics
        assert self.worker.read('agent/computer-probe/manifest.json') is None
        self.report['worker_diagnostics'] = diagnostics
        self.worker_unit = self.start(self.worker, 'worker', self.root + '/bin/botstral-agent', ['run', '--state-dir', self.root + '/agent'])
        worker = poll(lambda: self.owner('GET', '/v1/workers'), lambda values: len(values) == 1 and values[0]['phase'] == 'ready', 'production worker readiness', seconds=90)[0]
        assert self.worker.read('agent/computer-probe/manifest.json') is None
        self.worker_id = worker['id']
        self.session = self.owner('POST', '/v1/computer-sessions', {'bot_id': self.bot, 'worker_id': self.worker_id})
        self.session = poll(lambda: self.owner('GET', '/v1/computer-sessions/' + self.session['id']), lambda value: value['status'] == 'confirmed', 'confirmed session')
        if self.args.suite != 'helper':
            self.pair_clients()

    def pair_clients(self):
        self.credential = secrets.token_urlsafe(32)
        device_fingerprint = hashlib.sha256(b'botstral-owner-device-v1\0' + self.credential.encode()).hexdigest()
        self.invitation = self.owner('POST', '/v1/device-invitations', {'request': {'fingerprint': device_fingerprint, 'label': 'Acceptance protocol observer'}, 'lifetime_seconds': 600})
        self.open_socket(first=True)
        self.native = Native(self.args.native, self.output)
        request = self.native.call('begin')['view']['pairing']
        grant = self.owner('POST', '/v1/device-invitations', {'request': request, 'lifetime_seconds': 600})
        private(self.output / 'invitation.json', {'trust': self.trust, **{key: grant[key] for key in ('role', 'fingerprint', 'expires_at', 'invitation')}})
        candidate = self.native.call('stage', file=str(self.output / 'invitation.json'))['view']['candidate']
        paired = self.native.call('confirm', id=candidate['id'])
        assert paired['workspace'] is not None
        self.check('native_desktop_service_pairing_and_os_keychain', cursor=paired['workspace']['cursor'], leaf=self.trust['leaf'])

    @classmethod
    def resume(cls, args):
        self = cls.__new__(cls)
        try:
            self.load_resume(args)
        except BaseException:
            if hasattr(self, 'units'):
                self.cleanup()
            raise
        return self

    def load_resume(self, args):
        original = args.resume.read_bytes()
        source = json.loads(original)
        assert not source.get('cleanup_errors') and not source.get('native_cleanup_error')
        verified = {check['name'] for check in source['checks'] if check['verdict'] == 'VERIFIED'}
        assert 'manager_restart_preserves_original_routine_and_duplicate_trigger' in verified
        self.args, self.build = args, json.loads(args.build.read_text())
        self.output = args.output.resolve()
        self.output.mkdir(mode=0o700, parents=True, exist_ok=False)
        self.identifier = uuid.uuid4().hex[:12]
        self.root, self.workspace = source['root'], source['workspace']
        self.run_id = Path(self.root).name
        self.manager = Remote(self.build['hosts']['manager'], self.root)
        self.worker = Remote(self.build['hosts']['agent'], self.root)
        self.units, self.native, self.socket, self.token = [], None, None, None
        self.report = source
        self.report['continued_from'] = {'sha256': hashlib.sha256(original).hexdigest(), 'path': str(args.resume),
            'error': self.report.pop('error', None)}
        self.report['passed'] = False
        self.report['continuation_harness_sha256'] = {}
        for path in [Path(__file__), Path(__file__).with_name('first_acceptance_remote.py'),
                SOURCE / 'manager/tests/fixtures/m06-integrated/acceptance-server.mjs']:
            (self.output / (path.name + '.source')).write_bytes(path.read_bytes())
            self.report['continuation_harness_sha256'][str(path.relative_to(SOURCE))] = hashlib.sha256(path.read_bytes()).hexdigest()
        for name, value in source['context'].items():
            if name != 'ledger':
                setattr(self, name, value)
        ledger_path = source['context'].get('ledger') or source['continuation_budget']['ledger']
        for name in ('ca.pem', 'manager.pem'):
            (self.output / name).write_bytes((args.resume.parent / name).read_bytes())
        self.context = ssl.create_default_context(cafile=str(self.output / 'ca.pem'))
        helper = SOURCE / 'manager/tests/first_acceptance_remote.py'
        for remote in (self.manager, self.worker):
            remote.files({'first_acceptance_remote.py': helper.read_bytes()})
        actual_workspace = json.loads(self.worker.read('guest-workspace.json'))
        if actual_workspace != self.workspace:
            assert actual_workspace == self.root + '/workspace'
            self.report['workspace_relocation'] = {'from': self.workspace, 'to': actual_workspace,
                'reason': 'Selected backup roots require a private parent directory.'}
            self.workspace = actual_workspace
        manager_digest = source['binary_sha256']['manager']
        assert self.manager.call('hashes', files=['bin/botstral-manager'])['bin/botstral-manager'] == manager_digest
        assert self.worker.call('hashes', files=['bin/botstral-agent'])['bin/botstral-agent'] == source['binary_sha256']['worker']
        self.direct = '192.168.1.40:' + self.addresses['public'].rsplit(':', 1)[1]
        self.start_manager('manager-continuation')
        self.login()
        pending = self.observe()
        cancelled = []
        for run in pending['runs']:
            if run['phase']['type'] not in ('completed', 'stopped', 'cancelled', 'awaiting_attention'):
                self.owner('POST', '/v1/runs/' + run['id'] + '/cancel')
                cancelled.append(run['id'])
        prior = self.manager.call('provider_requests')
        new_calls = 0 if 'native_desktop_browser_file_exact_approval_and_verified_export' in verified else 2
        phase = str(uuid.uuid4())
        if new_calls:
            ledger = self.manager.call('reserve', path=ledger_path, run=phase, cap_microusd=1000000,
                previous_reserved_microusd=0, reservation_microusd=CALL_HOLD * new_calls)
            self.report['budget'] = ledger
        self.report.pop('reconciled_budget', None)
        self.report['continuation_budget'] = {'id': phase, 'prior_attempt_ids': [item['response']['id'] for item in prior],
            'maximum_new_calls': new_calls, 'new_reservation_microusd': new_calls * CALL_HOLD,
            'prior_calls_retained': True, 'cancelled_prior_runs': cancelled, 'ledger': ledger_path}
        self.unlock()
        self.start(self.manager, 'connector-continuation', self.root + '/bin/node', [self.root + '/sdk/acceptance-server.mjs', self.root])
        self.worker_unit = self.start(self.worker, 'worker-continuation', self.root + '/bin/botstral-agent', ['run', '--state-dir', self.root + '/agent'])
        poll(lambda: self.owner('GET', '/v1/workers'), lambda values: values[0]['phase'] == 'ready', 'continued worker', seconds=90)
        self.session = poll(lambda: self.owner('GET', '/v1/computer-sessions/' + self.session['id']),
            lambda value: value['status'] == 'confirmed', 'continued session', seconds=30)
        self.pair_clients()
        self.save()

    def worker_authority(self):
        results = []
        for method, path in [('PUT', '/v1/tasks/' + str(uuid.uuid4()) + '/approval'),
                ('POST', '/v1/device-invitations'), ('GET', '/v1/provider-keys/mistral'), ('GET', '/owner/v1/connect')]:
            value = self.worker.call('worker_http', method=method, path=path)
            assert value['status'] == (403 if path == '/owner/v1/connect' else 404), value
            results.append({'method': method, 'path': path, **value})
        self.check('worker_credentials_cannot_approve_enroll_owner_or_read_credentials', responses=results)

    def prepare_compaction(self):
        driver = SOURCE / 'manager/tests/fixtures/first-acceptance/compact.py'
        self.worker.files({'compact.py': driver.read_bytes()})
        pinned = json.loads(driver.with_name('pinned.json').read_text())
        self.compaction_runtime = self.worker.call('compaction_runtime', source_digests=pinned['sources'])
        if not self.args.recorded:
            probe = json.loads(self.args.probe.read_text())['compaction_probe']
            assert self.compaction_runtime['content_sha256'] == probe['runtime_digest'], 'runtime changed after recorded probe'
        self.worker.files({'agent/vibe.json': json.dumps({**{k: self.compaction_runtime[k] for k in ('runtime', 'directory', 'manifest')},
            'account': ACCOUNT, 'ceilings': PROCESS_LIMITS}, default=str).encode()})

    def helper_compaction(self):
        workspace = {'id': str(uuid.uuid4()), 'path': self.workspace}
        hold = ((self.configuration['policy']['context_tokens'] * 200000 + 999999) // 1000000
                + (OUTPUT * 800000 + 999999) // 1000000)
        deadline = int(time.time()) + 240
        def limits(calls):
            return {'deadline': deadline, 'max_turns': calls,
                'max_total_tokens': calls * (self.configuration['policy']['context_tokens'] + OUTPUT),
                'max_cost_microusd': calls * hold, 'max_output_tokens': OUTPUT,
                'max_operations': 1, 'max_concurrent_operations': 1}
        policy = {'workspace': workspace, 'runtime': self.compaction_runtime['runtime'], 'account': ACCOUNT,
            'resources': PROCESS_LIMITS, 'configuration_revision': self.configuration['revision'],
            'model_id': self.model, 'access': 'edit_files_as_guest_account', 'outputs': []}
        identity = str(uuid.uuid4())
        request = {'id': identity, 'bot': self.bot, 'session': self.session['id'],
            'configuration_revision': self.configuration['revision'],
            'prompt': 'Call the vibe tool exactly once with a new session and prompt: Retain the acceptance fixture context. Then finish.',
            'execution_policy': {'vibe': {'policy': policy, 'limits': limits(HELPER_ADMISSION_BOUND)},
                'computer': None, 'process': None}, 'limits': limits(CAMPAIGN_ADMISSION_BOUND)}
        self.report['helper_request'] = request
        self.save()
        self.owner('POST', '/v1/runs', request)
        run = poll(lambda: self.owner('GET', '/v1/runs/' + identity),
            lambda value: value['summary']['phase']['type'] in ('waiting_for_tool', 'completed', 'stopped', 'awaiting_attention'), 'helper coding proposal')
        assert run['summary']['phase']['type'] == 'waiting_for_tool', run
        task_id = run['summary']['phase']['task']
        self.approve(task_id)
        native = poll(lambda: self.worker.call('compaction_native', workspace=workspace['id']),
            lambda value: value is not None, 'recorded real compaction', seconds=90)
        self.report['helper_native'] = native
        active = self.manager.call('coding_evidence', task=task_id)
        task = self.owner('GET', '/v1/tasks/' + task_id)
        execution = task['command']['execution']
        self.report['helper_identity'] = {'task_id': task_id, 'execution_id': execution}
        active['worker_scope'] = self.worker.call('scope', execution=execution)
        active['worker_process'] = native['process']
        self.report['helper_active_lease'] = active
        self.save()
        from first_acceptance_compaction import validate_lease, verify_native
        try:
            verify_native(native)
            validate_lease(active, task_id, execution)
            self.report['helper_refusals'] = self.helper_policy(task_id, execution, native)
        finally:
            self.worker.call('compaction_release', workspace=workspace['id'])
            finished = self.task(task_id)
        assert finished['state'] == 'completed', finished
        parent = poll(lambda: self.owner('GET', '/v1/runs/' + identity),
            lambda value: value['summary']['phase']['type'] in ('completed', 'stopped', 'awaiting_attention'), 'parent helper settlement')
        assert parent['summary']['phase']['type'] == 'completed', parent
        self.report['helper_coding'] = self.manager.call('coding_evidence', task=task_id)
        self.report['helper_owner_calls'] = self.owner('GET', '/v1/tasks/' + task_id + '/coding-calls')
        self.report['helper_parent'] = parent
        if self.args.recorded:
            record = {'recorded_transport': True, 'offline_passed': True, 'native': native,
                'active_lease': active, 'task_id': task_id, 'execution_id': execution,
                'runtime_content_sha256': self.compaction_runtime['content_sha256']}
            self.report['compaction_probe'] = asdict(probe_compaction(record))
        self.save()

    def helper_policy(self, task, execution, native):
        results = []
        self.report['helper_refusals'] = results
        current = self.owner('GET', '/v1/inference-configurations/' + self.bot)
        for name, model in [('missing_acknowledgment', None), ('unconsented_preview', 'mistral-small-preview'),
                            ('unconsented_latest', 'mistral-small-latest')]:
            policy = json.loads(json.dumps(current['policy']))
            if model:
                policy['model_id'] = model
            else:
                del policy['acknowledgment']
            before = self.manager.call('attempt_inventory')
            response = self.manager.call('http', address=self.addresses['listen'], method='PUT', path='/v1/inference-configurations',
                token=self.token, body={'bot': self.bot, 'expected_revision': current['revision'], 'policy': policy})
            after = self.manager.call('attempt_inventory')
            results.append({'name': name, 'synthetic': True, 'status': response['status'],
                'request': {'bot': self.bot, 'expected_revision': current['revision'], 'policy': policy},
                'response': response['body'], 'before': before, 'after': after})
            self.save()
            assert response['status'] == 400 and before == after, response
        assert self.owner('GET', '/v1/inference-configurations/' + self.bot) == current
        for name in ('wrong_model', 'unsupported_tool'):
            body = json.loads(native['requests'][0]['body'])
            if name == 'wrong_model':
                body['model'] = 'mistral-small-latest'
            else:
                body['tools'][0]['function']['name'] = 'unsupported_acceptance_capability'
            before = self.manager.call('attempt_inventory')
            response = self.worker.call('worker_http', method='POST',
                path='/worker/v1/coding/' + task + '/' + execution + '/' + str(uuid.uuid4()), body=body)
            after = self.manager.call('attempt_inventory')
            results.append({'name': name, 'synthetic': True, 'request': body, 'status': response['status'],
                'response': response['body'], 'before': before, 'after': after})
            self.save()
            assert response['status'] == 409 and json.loads(response['body'])['error'] == 'invalid_message' and before == after, response
        request = self.process_request('unsupported-compaction-policy.txt')
        request['settings']['model'] = self.model
        request['work'] = {'type': 'unavailable'}
        before = self.manager.call('attempt_inventory')
        submitted = self.owner('POST', '/v1/tasks', request)
        refused = self.task(submitted['id'])
        after = self.manager.call('attempt_inventory')
        results.append({'name': 'unsupported_capability', 'synthetic': True, 'request': request,
            'status': 200, 'outcome': refused['outcome'], 'before': before, 'after': after})
        self.save()
        assert refused['state'] == 'failed' and refused['outcome']['code'] == 'unsupported_execution' and before == after, refused
        return results

    def helper_evidence(self):
        report = self.report
        probe = dict(report['compaction_probe']) if self.args.recorded else json.loads(self.args.probe.read_text())['compaction_probe']
        from first_acceptance_compaction import CompactionProbe
        probe['source_digests'] = tuple(tuple(item) for item in probe['source_digests'])
        ledger = report['reconciled_budget']
        reservation = next(item for item in report['budget']['reservations'] if item['run'] == self.run_id)
        retained = next(item for item in ledger['reservations'] if item['run'] == self.run_id)
        return asdict(CompactionEvidence(CompactionProbe(**probe), **report['helper_identity'],
            session_id=report['helper_native']['native_session'],
            coding_call_ids=tuple(c['id'] for c in report['helper_coding']['attempts']),
            native=encoded(report['helper_native']), coding=encoded(report['helper_coding']),
            owner_calls=encoded(report['helper_owner_calls']), active_lease=encoded(report['helper_active_lease']),
            parent_run=encoded(report['helper_parent']), policy_refusals=tuple(encoded(r) for r in report['helper_refusals']),
            budget_evidence=encoded({'cap_microusd': ledger['cap_microusd'], 'maximum_attempts': CAMPAIGN_ADMISSION_BOUND,
                'runtime_attempt_bound': RUNTIME_CALL_BOUND, 'reconciled': True,
                'run': self.run_id, 'snapshot_sha256': sha((self.output / 'budget-evidence.json').read_bytes()),
                'reserved_microusd': reservation['microusd'], 'retained_microusd': retained['microusd'], 'ledger': ledger}),
            provenance=encoded({'recorded_transport': self.args.recorded, 'production_binaries': report['production_binaries'],
                'binary_sha256': report['binary_sha256'], 'provider_base': 'https://api.eu.mistral.ai',
                'runtime_content_sha256': self.compaction_runtime['content_sha256'],
                'provider_source_sha256': sha((SOURCE / 'manager/src/manager/provider.rs').read_bytes()),
                'relay_source_sha256': sha((SOURCE / 'agent/vibe/runtime.py').read_bytes())}),
            cleanup=encoded({'errors': report.get('cleanup_errors', []), 'secret_scan': report['provider_secret_scan'],
                'scope': report['helper_scope_cleanup'],
                'temporary_provider_key_removed': report['temporary_provider_key_removed']})))

    def configure_provider(self):
        ssh(self.manager.host, ['mkdir', '-p', '-m', '700', self.args.campaign_directory], capture_output=True)
        campaign = self.args.campaign_directory + '/budget.json'
        self.ledger = campaign
        ledger = self.manager.call('reserve', path=campaign, run=self.run_id, cap_microusd=1000000,
            previous_reserved_microusd=0, reservation_microusd=CALL_HOLD * (CAMPAIGN_ADMISSION_BOUND if self.args.suite == 'helper' else PLANNED_CALLS))
        self.report['budget'] = ledger
        self.manager.files({'provider-key': b'synthetic-m04-provider-secret' if self.args.recorded else self.args.key_file.read_bytes()})
        self.manager.call('provision', address=self.addresses['listen'], token=self.token)
        now = int(time.time())
        config = self.owner('PUT', '/v1/inference-configurations', {'bot': self.bot, 'expected_revision': 0, 'policy': {
            'endpoint': 'eu', 'model_id': self.model, 'context_tokens': 32768 if self.args.recorded else CONTEXT,
            'acknowledgment': {'model_developer': 'mistralai', 'developer_terms_url': 'https://legal.mistral.ai/terms/api',
                'privacy_terms_url': 'https://legal.mistral.ai/terms/privacy-policy', 'acknowledged_at': now, 'release': {'type': 'stable'}},
            'prices': {'input_microusd_per_million_tokens': 200000, 'output_microusd_per_million_tokens': 800000,
                'effective_at': now, 'expires_at': now + 7200, 'billing': 'context_tokens_including_images_at_acknowledged_ceilings'}}})
        self.configuration = self.owner('POST', '/v1/inference-configurations/verify', {'bot': self.bot, 'revision': config['revision']})
        assert self.configuration['verification']['type'] == 'verified', self.configuration
        self.check('verified_live_eu_model_and_capped_acceptance_reservation', configuration=self.configuration, ledger=ledger)

    def process_request(self, name, sleep=0):
        deadline = int(time.time()) + 240
        return {'id': str(uuid.uuid4()), 'session_id': self.session['id'], 'conversation_id': None,
            'settings': {'model': 'unavailable', 'max_turns': 1, 'max_tokens': 1, 'deadline': deadline, 'max_cost_micros': 0},
            'work': {'type': 'process', 'spec': {'profile': 'linux_systemd_v1', 'account': ACCOUNT, 'executable': '/usr/bin/python3',
                'args': ['-c', 'from pathlib import Path;import time;Path(' + repr(self.workspace + '/' + name) + ').write_text("one");time.sleep(' + str(sleep) + ')'],
                'environment': {}, 'cwd': self.root, 'deadline': deadline, 'limits': PROCESS_LIMITS}}}

    def desktop_browser(self):
        self.worker.call('guest_write', files={'page.html': '<html><body><input id="value"><button id="save" onclick="document.getElementById(\'result\').textContent=document.getElementById(\'value\').value">Save</button><p id="result"></p></body></html>'})
        self.worker.files({'page.py': ('import http.server,json,os\nos.chdir(' + repr(self.workspace) + ')\nserver=http.server.ThreadingHTTPServer(("127.0.0.1",0),http.server.SimpleHTTPRequestHandler)\nopen(' + repr(self.root + '/page-port') + ',"w").write(str(server.server_port))\nserver.serve_forever()\n').encode()})
        self.worker.call('unlink', paths=['page-port'])
        self.start(self.worker, 'page', '/usr/bin/python3', [self.root + '/page.py'])
        port = poll(lambda: self.worker.read('page-port'), lambda value: value is not None, 'guest browser origin')
        proposal = {'operations': [{'type': 'navigate', 'url': 'http://127.0.0.1:' + port + '/page.html'},
            {'type': 'fill', 'selector': '#value', 'text': 'First acceptance browser proof'},
            {'type': 'click', 'selector': '#save'}, {'type': 'capture_text', 'selector': '#result', 'output': 'text'},
            {'type': 'write_file', 'path': self.workspace + '/proof.txt', 'text': 'First acceptance file proof'}], 'outputs': ['text']}
        prompt = 'Call the computer tool once with these exact arguments. The output slot is "text", not the filename "result.txt". After the tool result, reply with Complete. Arguments: ' + json.dumps(proposal)
        existing = {task['id'] for task in self.observe()['tasks']}
        self.native.call('send', input={'bot': self.bot, 'session': self.session['id'], 'text': prompt, 'account': ACCOUNT,
            'runtime': self.runtime, 'process': False, 'seconds': 120, 'turns': 2, 'tokens': 2 * (CONTEXT + OUTPUT),
            'cost_microusd': 2 * CALL_HOLD, 'output_tokens': OUTPUT})
        proposed = poll(self.observe, lambda value: any(task['id'] not in existing and task['approval']['binding'] is None and task['request']['work']['type'] == 'computer'
            for task in value['tasks']), 'native browser proposal', seconds=120)
        task = next(task for task in proposed['tasks'] if task['id'] not in existing and task['approval']['binding'] is None and task['request']['work']['type'] == 'computer')
        review = self.native.call('review', id=task['id'])['view']['review']
        assert review is not None
        assert task['request']['work']['spec']['operations'] == proposal['operations']
        assert review['task'] == task['id']
        self.browser_run = next(run['id'] for run in proposed['runs'] if run['phase'].get('task') == task['id'])
        assert self.worker.call('guest_read', name='proof.txt') is None
        self.native.call('approve', id=review['id'])
        completed = self.task(task['id'])
        assert completed['state'] == 'completed', completed
        completed = poll(lambda: self.owner('GET', '/v1/tasks/' + task['id']),
            lambda value: any(output['state']['type'] == 'available' for output in value['outputs']), 'native browser artifact')
        artifact = next(output['state'] for output in completed['outputs'] if output['declaration']['slot'] == 'text')
        assert artifact['type'] == 'available'
        self.native.call('export', task=task['id'], artifact=artifact['id'], name='browser-proof.txt')
        assert (self.output / 'browser-proof.txt').read_text() == 'First acceptance browser proof'
        assert self.worker.call('guest_read', name='proof.txt') == 'First acceptance file proof'
        self.browser_task = completed
        self.browser_artifact = artifact
        self.check('native_desktop_browser_file_exact_approval_and_verified_export', task=completed,
            exported_sha256=hashlib.sha256((self.output / 'browser-proof.txt').read_bytes()).hexdigest())

    def desktop_cancel(self):
        request = self.process_request('cancel.txt', sleep=150)
        pending = self.call({'method': 'submit_task', 'input': request})
        review = self.native.call('review', id=pending['id'])['view']['review']
        self.native.call('approve', id=review['id'])
        self.task(pending['id'], terminal=False)
        execution = pending['command']['execution']
        poll(lambda: self.worker.read('agent/processes/' + execution + '/ready.json'), lambda value: value is not None, 'cancellable process')
        scope = self.worker.call('scope', execution=execution)
        assert scope['pids']
        self.native.call('cancel', id=pending['id'])
        assert self.task(pending['id'])['state'] == 'cancelled'
        empty = poll(lambda: self.worker.call('scope', execution=execution, observed=scope['pids']),
            lambda value: not value['pids'] and not value['survivors'], 'native cancellation process absence')
        self.check('native_desktop_cancellation_confirms_empty_scope', task=pending['id'], before=scope, after=empty)

    def worker_effect_recovery(self):
        retained = self.report.get('worker_fault')
        if retained:
            repaired = self.owner('GET', '/v1/tasks/' + retained['task'])
            assert repaired['state'] == 'awaiting_attention', repaired
            assert repaired['outcome']['type'] == 'stopped' and repaired['effect_evidence']
            self.report.setdefault('prior_worker_faults', []).append({**retained, 'after_startup_repair': repaired})
            self.save()
        name = 'lost-' + self.identifier + '.txt'
        deadline = int(time.time()) + 300
        task = self.call({'method': 'submit_task', 'input': {'id': str(uuid.uuid4()), 'session_id': self.session['id'], 'conversation_id': None,
            'settings': {'model': MODEL, 'max_turns': 1, 'max_tokens': 1, 'deadline': deadline, 'max_cost_micros': 0},
            'work': {'type': 'connector', 'spec': {'operation': {'type': 'call', 'proposal': {'tool': self.binding,
                'arguments': {'name': name, 'text': 'One effect before worker disconnect.'}}}, 'runner': self.runner,
                'deadline': deadline, 'result': {'slot': 'connector', 'name': 'connector.json', 'media_type': 'text_plain', 'max_bytes': 4096}}}}})
        self.approve(task['id'])
        self.manager.files({name + '.release': b''})
        poll(self.effects, lambda values: any(value['name'] == name for value in values), 'external effect before completion')
        before = self.owner('GET', '/v1/tasks/' + task['id'])
        assert before['state'] == 'running' and before['outcome'] is None
        journal_before = self.worker.call('journal')
        generation = self.owner('GET', '/v1/workers')[0]['generation']
        self.report['worker_fault'] = {'task': task['id'], 'receipt': name, 'before': before, 'journal_before': journal_before,
            'generation': generation, 'method': 'pause main worker until the manager closes the idle WSS connection'}
        self.save()
        self.worker.run(['systemctl', '--user', 'kill', '--signal=SIGSTOP', '--kill-whom=main', self.worker_unit])
        try:
            disconnected = poll(lambda: self.owner('GET', '/v1/workers'),
                lambda values: values[0]['phase'] == 'disconnected', 'actual worker connection loss', seconds=45)
            self.report['worker_fault']['disconnected'] = disconnected
            self.save()
        finally:
            self.worker.run(['systemctl', '--user', 'kill', '--signal=SIGCONT', '--kill-whom=main', self.worker_unit])
        self.manager.files({name + '.return': b''})
        poll(lambda: self.owner('GET', '/v1/workers'), lambda values: values[0]['phase'] == 'ready'
            and values[0]['generation'] > generation, 'worker recovery', seconds=90)
        recovered = self.task(task['id'])
        assert recovered['state'] in ('completed', 'awaiting_attention'), recovered
        assert recovered['command']['execution'] == task['command']['execution']
        ledger = self.owner('GET', '/v1/tasks/' + task['id'] + '/connector-calls')
        assert len(ledger) == 1
        journal_after = self.worker.call('journal')
        assert journal_after == journal_before
        expected = self.effects()
        assert len([effect for effect in expected if effect['name'] == name]) == 1
        self.stable_effects(expected)
        self.check('worker_disconnect_after_effect_reconciles_without_reexecution', before=before, after=recovered,
            connector_calls=ledger, journal_before=journal_before, journal_after=journal_after, effects=expected)

    def retention_recovery(self):
        before = self.native.call('suspend')['workspace']['cursor']
        conversation = getattr(self, 'history_conversation', str(uuid.uuid4()))
        messages = getattr(self, 'history_messages', [{'id': str(uuid.uuid4()), 'conversation': conversation,
            'text': 'Retention proof ' + str(index)} for index in range(160)])
        self.history_conversation, self.history_messages = conversation, messages
        self.save()
        self.owner('POST', '/v1/conversations', {'id': conversation, 'bot': self.bot})
        requests = [{'address': self.addresses['listen'], 'method': 'POST', 'path': '/v1/messages', 'token': self.token, 'body': message}
            for message in messages]
        replies = self.manager.call('http_batch', requests=requests)
        assert all(reply['status'] == 200 for reply in replies)
        revision = self.permission['revision']
        requests = [{'address': self.addresses['listen'], 'method': 'PUT', 'path': '/v1/bots/' + self.bot + '/permission',
            'token': self.token, 'body': {'expected_revision': revision + index, 'allow': True}} for index in range(160)]
        replies = self.manager.call('http_batch', requests=requests)
        assert all(reply['status'] == 200 for reply in replies)
        self.permission = replies[-1]['body']
        self.socket.close()
        self.socket = None
        self.open_socket()
        reply = self.call({'method': 'resume', 'cursor': before, 'limit': 64})
        assert reply['type'] == 'snapshot' and reply['cursor'] > before + 128
        self.native.call('resume')
        recovered = poll(lambda: self.native.call('inspect'), lambda value: value['workspace'] is not None and
            len([message for message in value['workspace']['state']['messages'] if message['conversation'] == conversation]) == len(messages),
            'native full snapshot after actual retention gap', seconds=60)
        actual = [message for message in recovered['workspace']['state']['messages'] if message['conversation'] == conversation]
        assert sorted(actual, key=lambda value: value['id']) == sorted(messages, key=lambda value: value['id'])
        self.history_conversation = conversation
        self.history_messages = messages
        self.check('native_desktop_snapshot_after_actual_event_retention_gap', disconnected_cursor=before,
            recovered_cursor=recovered['workspace']['cursor'], replay_type=reply['type'], messages=len(actual))

    def approval_authority(self):
        self.stop(self.worker, self.worker_unit)
        names = ('denied.txt', 'altered.txt', 'expired.txt', 'revoked.txt')
        requests = [self.process_request(name) for name in names]
        tasks = [self.owner('POST', '/v1/tasks', request) for request in requests]
        permission = self.owner('PUT', '/v1/bots/' + self.bot + '/permission', {'expected_revision': self.permission['revision'], 'allow': False})
        self.owner('PUT', '/v1/tasks/' + tasks[0]['id'] + '/approval', {'expires_at': int(time.time()) + 120}, status=409)
        self.permission = self.owner('PUT', '/v1/bots/' + self.bot + '/permission', {'expected_revision': permission['revision'], 'allow': True})
        altered = self.owner('GET', '/v1/tasks/' + tasks[1]['id'])
        command = json.loads(json.dumps(altered['command']))
        command['work']['spec']['args'][-1] += ';print("changed")'
        refusal = self.socket.call({'method': 'grant_reviewed_approval', 'id': altered['id'], 'command': command,
            'permission_revision': self.permission['revision'], 'approval': altered['approval'], 'expires_at': int(time.time()) + 120})
        assert refusal['type'] == 'error' and refusal['code']['type'] == 'conflict', refusal
        expiry = int(time.time()) + 2
        self.owner('PUT', '/v1/tasks/' + tasks[2]['id'] + '/approval', {'expires_at': expiry})
        self.approve(tasks[3]['id'])
        stale = self.owner('GET', '/v1/tasks/' + tasks[3]['id'])
        self.owner('DELETE', '/v1/tasks/' + tasks[3]['id'] + '/approval')
        revoked = self.socket.call({'method': 'grant_reviewed_approval', 'id': stale['id'], 'command': stale['command'],
            'permission_revision': self.permission['revision'], 'approval': stale['approval'], 'expires_at': int(time.time()) + 120})
        assert revoked['type'] == 'error' and revoked['code']['type'] == 'conflict', revoked
        while int(time.time()) <= expiry:
            time.sleep(.1)
        self.worker_unit = self.start(self.worker, 'worker-approval', self.root + '/bin/botstral-agent', ['run', '--state-dir', self.root + '/agent'])
        poll(lambda: self.owner('GET', '/v1/workers'), lambda values: values[0]['phase'] == 'ready', 'worker for stale authority', seconds=90)
        expected = self.effects()
        self.stable_effects(expected)
        for name, request in zip(names, requests):
            assert self.worker.call('guest_read', name=name) is None
            current = self.owner('GET', '/v1/tasks/' + request['id'])
            assert not current['approval']['consumed']
            self.owner('POST', '/v1/tasks/' + request['id'] + '/cancel')
            assert self.task(request['id'])['state'] == 'cancelled'
        request = self.process_request('consumed.txt')
        task = self.owner('POST', '/v1/tasks', request)
        stale = self.owner('GET', '/v1/tasks/' + task['id'])
        self.approve(task['id'])
        completed = self.task(task['id'])
        assert completed['state'] == 'completed' and completed['approval']['consumed']
        consumed = self.socket.call({'method': 'grant_reviewed_approval', 'id': stale['id'], 'command': stale['command'],
            'permission_revision': self.permission['revision'], 'approval': stale['approval'], 'expires_at': int(time.time()) + 120})
        assert consumed['type'] == 'error' and consumed['code']['type'] == 'conflict', consumed
        self.check('denied_altered_expired_consumed_and_revoked_authority_cannot_execute_stale_work',
            altered=refusal, revoked=revoked, consumed=consumed, blocked_tasks=[task['id'] for task in tasks], completed=completed)

    def model_policy(self):
        browser = poll(lambda: self.owner('GET', '/v1/runs/' + self.browser_run),
            lambda value: value['summary']['phase']['type'] in ('completed', 'stopped', 'awaiting_attention'), 'browser model completion')
        assert browser['summary']['phase']['type'] == 'completed'
        current = self.owner('GET', '/v1/inference-configurations/' + self.bot)
        for model in ('mistral-small-latest', 'mistral-small-preview'):
            policy = json.loads(json.dumps(current['policy']))
            policy['model_id'] = model
            self.owner('PUT', '/v1/inference-configurations', {'bot': self.bot,
                'expected_revision': current['revision'], 'policy': policy}, status=400)
        missing_acknowledgment = json.loads(json.dumps(current['policy']))
        del missing_acknowledgment['acknowledgment']
        self.owner('PUT', '/v1/inference-configurations', {'bot': self.bot,
            'expected_revision': current['revision'], 'policy': missing_acknowledgment}, status=400)
        assert self.owner('GET', '/v1/inference-configurations/' + self.bot) == current
        evidence = self.manager.call('provider_requests')
        assert evidence and all(item['request']['model'] == MODEL for item in evidence)
        assert all(item['endpoint'] == 'eu' for item in evidence)
        assert all(item['response']['phase']['type'] == 'responded' for item in evidence)
        request = self.process_request('unavailable.txt')
        request['work'] = {'type': 'unavailable'}
        unavailable = self.call({'method': 'submit_task', 'input': request})
        failed = self.task(unavailable['id'])
        assert failed['state'] == 'failed' and failed['outcome']['code'] == 'unsupported_execution'
        visible = poll(lambda: self.native.call('inspect'), lambda value: value['workspace'] is not None and
            any(task['id'] == unavailable['id'] and task['state'] == 'failed' for task in value['workspace']['state']['tasks']),
            'native unsupported execution blocker')
        assert self.manager.call('provider_requests') == evidence
        self.check('actual_eu_model_requests_and_preview_latest_configuration_blockers', configuration=current,
            requests=evidence, missing_privacy_acknowledgment_rejected=True, preview_without_consent_rejected=True,
            unsupported_execution=failed, native_cursor=visible['workspace']['cursor'], helpers_used=False,
            helper_capabilities_unverified=True)

    def backup_restore(self):
        documents = self.call({'method': 'workspace'})['state']['documents']
        memory = next((document for document in documents if document['bot'] == self.bot and document['kind'] == 'memory'), None)
        if memory is None:
            memory = self.owner('PUT', '/v1/documents', {'bot': self.bot, 'kind': 'memory', 'expected_revision': 0,
                'sources': [{'type': 'message', 'id': self.history_messages[0]['id']}], 'markdown': 'First acceptance retained memory.'})
        assert memory['markdown'] == 'First acceptance retained memory.'
        connection_before = self.owner('GET', '/v1/connectors/' + self.connection['id'])
        workspace_before = self.call({'method': 'workspace'})
        journal_before = self.worker.call('journal')
        proof_before = self.worker.call('guest_read', name='proof.txt')
        self.close_clients()
        self.stop(self.worker, self.worker_unit)
        for remote, unit, _ in list(reversed(self.units)):
            if remote is self.worker:
                self.stop(remote, unit)
        self.stop(self.manager, self.manager_unit)
        for component, remote, state in [('manager', self.manager, 'state'), ('agent', self.worker, 'agent')]:
            backup = self.root + '/' + component + '-backup-' + self.identifier
            self.report.setdefault('backup_attempts', []).append({'component': component, 'backup': backup})
            self.save()
            arguments = ['/usr/bin/python3', self.root + '/src/' + component + '/packaging/backup.py', 'create',
                '--state-dir', self.root + '/' + state, '--destination', backup]
            if component == 'agent':
                roots = [{'label': name, 'kind': kind, 'path': self.root + '/' + name} for name, kind in
                    [('certs', 'certificate'), ('tools', 'tool'), ('browser-runtime', 'runtime')]]
                roots.append({'label': 'workspace', 'kind': 'workspace', 'path': self.workspace})
                self.worker.files({'recovery-plan.json': json.dumps({'roots': roots}).encode()})
                arguments += ['--plan', self.root + '/recovery-plan.json']
            remote.run(arguments, timeout=240)
            remote.run(['/usr/bin/python3', self.root + '/src/' + component + '/packaging/backup.py',
                'verify', '--backup', backup], timeout=240)
        self.manager.call('rename', source='state', destination='state-retired-' + self.identifier)
        self.manager.run(['/usr/bin/python3', self.root + '/src/manager/packaging/backup.py', 'restore',
            '--backup', self.root + '/manager-backup-' + self.identifier, '--destination', self.root + '/state'], timeout=240)
        for name in ('agent', 'certs', 'tools', 'browser-runtime'):
            self.worker.call('rename', source=name, destination=name + '-retired-' + self.identifier)
        self.worker.run(['mv', self.workspace, self.workspace + '-retired-' + self.identifier])
        self.worker.run(['/usr/bin/python3', self.root + '/src/agent/packaging/backup.py', 'restore',
            '--backup', self.root + '/agent-backup-' + self.identifier, '--destination', self.root + '/agent', '--original-instance-retired'], timeout=240)
        assert self.worker.call('journal') == journal_before
        assert self.worker.call('guest_read', name='proof.txt') == proof_before
        self.start_manager('manager-restored')
        self.login()
        assert self.owner('GET', '/v1/status')['vault'] == 'locked'
        self.unlock()
        provider = self.owner('POST', '/v1/inference-configurations/verify', {'bot': self.bot, 'revision': self.configuration['revision']})
        assert provider['verification']['type'] == 'verified'
        assert self.owner('GET', '/v1/connectors/' + self.connection['id']) == connection_before
        self.worker_unit = self.start(self.worker, 'worker-restored', self.root + '/bin/botstral-agent', ['run', '--state-dir', self.root + '/agent'])
        poll(lambda: self.owner('GET', '/v1/workers'), lambda values: values[0]['phase'] == 'ready', 'restored worker', seconds=90)
        self.open_socket()
        deadline = int(time.time()) + 180
        restored_task = self.owner('POST', '/v1/tasks', {'id': str(uuid.uuid4()), 'session_id': self.session['id'], 'conversation_id': None,
            'settings': {'model': MODEL, 'max_turns': 1, 'max_tokens': 1, 'deadline': deadline, 'max_cost_micros': 0},
            'work': {'type': 'connector', 'spec': {'operation': {'type': 'call', 'proposal': {'tool': self.binding,
                'arguments': {'name': 'restored.txt', 'text': 'Restored connection credential.'}}}, 'runner': self.runner,
                'deadline': deadline, 'result': {'slot': 'connector', 'name': 'connector.json', 'media_type': 'text_plain', 'max_bytes': 4096}}}})
        self.approve(restored_task['id'])
        self.manager.files({'restored.txt.release': b'', 'restored.txt.return': b''})
        assert self.task(restored_task['id'])['state'] == 'completed'
        assert self.manager.read('restored.txt') == 'Restored connection credential.'
        after = self.call({'method': 'workspace'})
        for name in ('messages', 'documents'):
            assert after['state'][name] == workspace_before['state'][name]
        assert any(document == memory for document in after['state']['documents'])
        artifact = self.manager.call('artifact', address=self.addresses['listen'], token=self.token,
            path='/v1/tasks/' + self.browser_task['id'] + '/artifacts/' + self.browser_artifact['id'])
        assert artifact['sha256'] == self.browser_artifact['content']['sha256']
        self.native.call('restore')
        native = poll(lambda: self.native.call('inspect'), lambda value: value['workspace'] is not None and
            any(message['id'] == self.history_messages[0]['id'] for message in value['workspace']['state']['messages']),
            'native keychain restore after manager and Computer recovery', seconds=60)
        self.check('offline_backup_restores_history_memory_credentials_artifacts_and_computer_state',
            memory=memory, artifact=artifact, worker_journal=journal_before, guest_proof=proof_before,
            native_cursor=native['workspace']['cursor'], retained_connection=connection_before,
            restored_credential_task=restored_task['id'], provider_verification=provider,
            selected_worker_roots=['agent', 'workspace', 'certs', 'tools', 'browser-runtime'], original_instance_retired=True)

    def configure_connector(self):
        self.bearer = secrets.token_urlsafe(32)
        self.manager.files({'bearer': self.bearer.encode(), 'sdk/acceptance-server.mjs':
            (SOURCE / 'manager/tests/fixtures/m06-integrated/acceptance-server.mjs').read_bytes()})
        self.start(self.manager, 'connector', self.root + '/bin/node', [self.root + '/sdk/acceptance-server.mjs', self.root])
        port = json.loads(poll(lambda: self.manager.read('https.json'), lambda value: value is not None, 'HTTPS connector'))['port']
        connection = self.owner('POST', '/v1/connectors', {'id': str(uuid.uuid4()), 'label': 'Acceptance SDK receipts',
            'transport': {'type': 'remote_http', 'endpoint': 'https://192.168.1.40:' + str(port) + '/mcp'}})
        connection = self.owner('PUT', '/v1/connectors/' + connection['id'] + '/secret',
            {'expected_revision': 1, 'secret': {'type': 'bearer', 'value': self.bearer}})
        task = str(uuid.uuid4())
        self.owner('POST', '/v1/connectors/discover', {'task': task, 'session': self.session['id'],
            'expected': connection['version'], 'runner': self.runner, 'deadline': int(time.time()) + 180})
        self.approve(task)
        assert self.task(task)['state'] == 'completed'
        self.connection = self.owner('GET', '/v1/connectors/' + connection['id'])
        self.binding = self.owner('PUT', '/v1/bots/' + self.bot + '/connectors', {'connection': self.connection['version'],
            'catalog_revision': self.connection['catalog_revision'], 'expected_revision': 0, 'tools': ['write_receipt']})['tools'][0]
        self.check('authenticated_real_sdk_https_connector_discovery_with_exact_approval', connection=self.connection, task=task)

    def publish(self, publisher, event, expected=202):
        import http.client
        host, port = self.direct.rsplit(':', 1)
        connection = http.client.HTTPSConnection(host, int(port), context=self.context, timeout=20)
        try:
            connection.request('POST', '/events/v1/publishers/' + publisher, json.dumps(event),
                {'Authorization': 'Bearer ' + self.publisher_credential, 'Content-Type': 'application/json'})
            response = connection.getresponse()
            value = json.loads(response.read())
            assert response.status == expected, (response.status, value)
            return value
        finally:
            connection.close()

    def p03(self):
        self.socket.close()
        self.socket = None
        self.open_socket()
        publisher, routine = str(uuid.uuid4()), str(uuid.uuid4())
        self.publisher_credential = secrets.token_urlsafe(48)
        self.routine({'type': 'set_budget', 'budget': {'max_runs': 1, 'max_tokens': 2 * (CONTEXT + OUTPUT), 'max_cost_microusd': 2 * CALL_HOLD}})
        self.routine({'type': 'install_publisher', 'id': publisher, 'bot': self.bot, 'name': 'Acceptance publisher',
            'topics': ['acceptance.ready'], 'credential_sha256': hashlib.sha256(self.publisher_credential.encode()).hexdigest()})
        self.routine({'type': 'create', 'id': routine, 'definition': {'bot': self.bot, 'name': 'Closed-client receipt',
            'trigger': {'type': 'event', 'publisher': publisher, 'topic': 'acceptance.ready', 'equals': {}},
            'catchup_seconds': 60, 'wakeups': None, 'template': {'session': self.session['id'], 'session_revision': self.session['revision'],
                'configuration_revision': self.configuration['revision'], 'permission_revision': self.permission['revision'],
                'prompt': 'Use the only available connector tool exactly once. Set name to "routine.txt" and text to "First acceptance routine." After the tool result, reply with Receipt complete. Do not call another tool.',
                'execution_policy': {'computer': None, 'process': None, 'connectors': {'tools': [self.binding], 'max_calls': 1,
                    'max_result_bytes': 4096, 'runner': self.runner}}, 'duration_seconds': 600,
                'max_turns': 2, 'max_total_tokens': 2 * (CONTEXT + OUTPUT), 'max_cost_microusd': 2 * CALL_HOLD,
                'max_output_tokens': OUTPUT, 'max_operations': 1}}})
        self.close_clients()
        event = {'id': 'first-acceptance-one', 'timestamp': int(time.time()), 'topic': 'acceptance.ready', 'fields': {}}
        receipt = self.publish(publisher, event)
        observed = poll(self.observe, lambda value: any(task['approval']['binding'] is None and task['request']['work']['type'] == 'connector'
            for task in value['tasks']),
            'autonomous live routine exact action', seconds=120)
        occurrence = next(value for value in observed['occurrences'] if value['routine'] == routine)
        assert occurrence['run'] is not None
        task = next(value for value in observed['tasks'] if value['approval']['binding'] is None and value['request']['work']['type'] == 'connector')
        assert task['request']['work']['spec']['operation']['proposal']['arguments'] == {'name': 'routine.txt', 'text': 'First acceptance routine.'}, task
        assert not self.effects()
        self.open_socket()
        assert len(self.call({'method': 'routine_history', 'id': routine, 'before': None, 'limit': 32})) == 1
        self.approve(task['id'])
        self.close_clients()
        self.manager.files({'routine.txt.release': b''})
        poll(self.effects, lambda values: len(values) == 1, 'one external connector effect', seconds=30)
        self.manager.files({'routine.txt.return': b''})
        settled = poll(self.observe, lambda value: any(run['id'] == occurrence['run'] and run['phase']['type'] == 'completed'
            for run in value['runs']), 'closed-client routine completion', seconds=120)
        self.check('p03_closed_client_live_inference_exact_approval_and_one_effect', occurrence=occurrence, task=task['id'], receipt=receipt,
            effect=self.effects(), completed=settled['runs'], explicit_approval_between_closed_client_windows=True)
        self.open_socket()
        self.native.call('resume')
        deadline = int(time.time()) + 240
        active = self.owner('POST', '/v1/tasks', {'id': str(uuid.uuid4()), 'session_id': self.session['id'], 'conversation_id': None,
            'settings': {'model': 'unavailable', 'max_turns': 1, 'max_tokens': 1, 'deadline': deadline, 'max_cost_micros': 0},
            'work': {'type': 'process', 'spec': {'profile': 'linux_systemd_v1', 'account': ACCOUNT, 'executable': '/usr/bin/python3',
                'args': ['-c', 'import time;time.sleep(150)'], 'environment': {}, 'cwd': self.root,
                'deadline': deadline, 'limits': PROCESS_LIMITS}}})
        self.approve(active['id'])
        self.task(active['id'], terminal=False)
        execution = active['command']['execution']
        poll(lambda: self.worker.read('agent/processes/' + execution + '/ready.json'), lambda value: value is not None, 'original process scope')
        scope_before = self.worker.call('scope', execution=execution)
        journal_before = self.worker.call('journal')
        assert scope_before['pids']
        self.close_clients()
        before = self.observe()
        restart = self.stop(self.manager, self.manager_unit)
        assert restart['exited']['ExecMainStatus'] == '0'
        self.start_manager('manager-restarted')
        self.login()
        status = self.owner('GET', '/v1/status')
        assert status['vault'] == 'locked'
        self.unlock()
        poll(lambda: self.owner('GET', '/v1/workers'), lambda values: values[0]['phase'] == 'ready', 'worker reconnect after manager restart', seconds=60)
        self.open_socket()
        history = self.call({'method': 'routine_history', 'id': routine, 'before': None, 'limit': 32})
        assert len(history) == 1 and history[0]['id'] == occurrence['id'] and history[0]['run'] == occurrence['run']
        assert self.publish(publisher, event) == receipt
        self.stable_effects(self.effects())
        after = self.observe()
        assert [run['id'] for run in before['runs']] == [run['id'] for run in after['runs']]
        assert len(self.effects()) == 1
        run = self.owner('GET', '/v1/runs/' + occurrence['run'])
        self.native.call('resume')
        snapshot = poll(lambda: self.native.call('inspect'), lambda value: value['workspace'] is not None and
            any(entry['summary']['id'] == occurrence['run'] for entry in value['workspace']['runs']), 'native routine recovery', seconds=60)
        private(self.output / 'routine-recovered.json', snapshot)
        recovered_task = self.owner('GET', '/v1/tasks/' + active['id'])
        assert recovered_task['command']['execution'] == active['command']['execution']
        scope_after = poll(lambda: self.worker.call('scope', execution=execution, observed=scope_before['pids']),
            lambda value: not value['pids'] and not value['survivors'], 'original scope stopped after disconnect', seconds=30)
        journal_after = self.worker.call('journal')
        assert journal_after == journal_before
        interrupted = self.task(active['id'])
        assert interrupted['state'] == 'awaiting_attention', interrupted
        self.owner('POST', '/v1/tasks/' + active['id'] + '/cancel')
        assert self.task(active['id'])['state'] == 'cancelled'
        self.check('manager_restart_preserves_original_routine_and_duplicate_trigger', history=history, run=run,
            locked_startup=status['vault'], task_before_restart=before['tasks'], task_after_restart=after['tasks'],
            scope_before=scope_before, scope_after=scope_after, journal_before=journal_before, journal_after=journal_after)

    def cleanup(self):
        if self.socket:
            self.socket.close()
            self.socket = None
        if self.native:
            try:
                self.native.close()
            except Exception as error:
                self.report['native_cleanup_error'] = type(error).__name__
                try:
                    self.native.process.terminate()
                    self.native.process.wait(timeout=15)
                except Exception as termination_error:
                    self.report.setdefault('cleanup_errors', []).append({'native_termination': type(termination_error).__name__})
        for remote, unit, _ in list(reversed(self.units)):
            try:
                self.stop(remote, unit)
            except Exception as error:
                self.report.setdefault('cleanup_errors', []).append({'unit': unit, 'error': type(error).__name__})
        if self.report.get('helper_identity'):
            try:
                observed = self.report.get('helper_active_lease', {}).get('worker_scope', {}).get('pids', [])
                scope = poll(lambda: self.worker.call('scope', execution=self.report['helper_identity']['execution_id'], observed=observed),
                    lambda value: not value['pids'] and not value['survivors'], 'helper process cleanup')
                self.report['helper_scope_cleanup'] = scope
            except Exception as error:
                self.report.setdefault('cleanup_errors', []).append({'helper_scope': type(error).__name__})
        try:
            scan = self.manager.call('scan_provider')
            self.report['provider_secret_scan'] = scan
            if scan['verdict'] != 'NOT SCANNED':
                assert scan['files'] > 0 and not scan['leaks'], scan
        except Exception as error:
            self.report.setdefault('cleanup_errors', []).append({'provider_scan': type(error).__name__})
        finally:
            try:
                self.manager.call('unlink', paths=['provider-key'])
                assert self.manager.read('provider-key') is None
                self.report['temporary_provider_key_removed'] = True
            except Exception as error:
                self.report.setdefault('cleanup_errors', []).append({'provider_cleanup': type(error).__name__})
        self.save()
        if hasattr(self, 'ledger') and not self.report.get('cleanup_errors'):
            snapshot = self.output / 'budget-evidence.json'
            snapshot.write_bytes((self.output / 'results.json').read_bytes())
            snapshot.chmod(0o600)
            evidence = hashlib.sha256(snapshot.read_bytes()).hexdigest()
            try:
                ledger = self.manager.call('reconcile_budget', root=self.root, run=self.run_id, ledger=self.ledger, evidence_sha256=evidence)
                self.report['reconciled_budget'] = ledger
            except Exception as error:
                self.report.setdefault('cleanup_errors', []).append({'budget_reconciliation': type(error).__name__})
            self.save()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--build', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--key-file', type=Path)
    parser.add_argument('--resume', type=Path)
    parser.add_argument('--native', type=Path)
    parser.add_argument('--campaign-directory', default='/home/botstral/first-acceptance-20261028')
    parser.add_argument('--suite', choices=['p03', 'full', 'helper'], default='full')
    parser.add_argument('--recorded', action='store_true')
    parser.add_argument('--recorded-mode', choices=['primary', 'fallback', 'empty'], default='primary')
    parser.add_argument('--probe', type=Path)
    args = parser.parse_args()
    if args.recorded and (args.suite != 'helper' or args.key_file or args.resume):
        parser.error('--recorded requires a fresh helper suite without a provider key')
    if args.suite == 'helper':
        if args.resume or args.campaign_directory == '/home/botstral/first-acceptance-20261028':
            parser.error('helper requires a fresh fixture and a separately named campaign directory')
        if not args.recorded:
            if not args.probe:
                parser.error('live helper requires --probe from the recorded worker lease suite')
            probe = json.loads(args.probe.read_text())
            probe_compaction(json.loads(probe['compaction_probe']['recorded_evidence']))
            from first_acceptance_compaction import decode_evidence, verify_helper
            verify_helper(decode_evidence(probe['compaction_evidence']), recorded=True)
            if probe.get('offline_passed') is not True or probe.get('cleanup_errors') or not probe.get('reconciled_budget'):
                parser.error('recorded probe cleanup and reconciliation must pass')
    if not args.recorded and not args.native:
        parser.error('--native is required for production binary provenance')
    if not args.resume and not args.key_file and not args.recorded:
        parser.error('--key-file is required for a new fixture')
    os.umask(0o077)
    acceptance = Acceptance.resume(args) if args.resume else Acceptance(args)
    try:
        if not args.resume:
            acceptance.prepare()
            acceptance.worker_authority()
            acceptance.configure_provider()
            if args.suite == 'helper':
                acceptance.helper_compaction()
            else:
                acceptance.configure_connector()
                acceptance.p03()
        if args.suite == 'full':
            steps = [('native_desktop_browser_file_exact_approval_and_verified_export', acceptance.desktop_browser),
                ('native_desktop_cancellation_confirms_empty_scope', acceptance.desktop_cancel),
                ('worker_disconnect_after_effect_reconciles_without_reexecution', acceptance.worker_effect_recovery),
                ('native_desktop_snapshot_after_actual_event_retention_gap', acceptance.retention_recovery),
                ('denied_altered_expired_consumed_and_revoked_authority_cannot_execute_stale_work', acceptance.approval_authority),
                ('actual_eu_model_requests_and_preview_latest_configuration_blockers', acceptance.model_policy),
                ('offline_backup_restores_history_memory_credentials_artifacts_and_computer_state', acceptance.backup_restore)]
            for name, operation in steps:
                if name not in {check['name'] for check in acceptance.report['checks'] if check['verdict'] == 'VERIFIED'}:
                    operation()
            if args.resume:
                current = acceptance.manager.call('provider_requests')
                previous = set(acceptance.report['continuation_budget']['prior_attempt_ids'])
                added = [item['response']['id'] for item in current if item['response']['id'] not in previous]
                assert len(added) <= acceptance.report['continuation_budget']['maximum_new_calls']
                acceptance.report['continuation_budget']['new_attempt_ids'] = added
    except Exception as error:
        acceptance.report['error'] = type(error).__name__ + ' ' + str(error)
    finally:
        acceptance.cleanup()
    required = FIRST_ACCEPTANCE_CHECKS
    verified = {check['name'] for check in acceptance.report['checks'] if check['verdict'] == 'VERIFIED'}
    acceptance.report['missing'] = sorted(required - verified) if args.suite != 'helper' else []
    if args.suite == 'helper' and not acceptance.report.get('error'):
        try:
            acceptance.report['compaction_evidence'] = acceptance.helper_evidence()
            if args.recorded:
                from first_acceptance_compaction import decode_evidence, verify_helper
                acceptance.report['recorded_helper'] = verify_helper(decode_evidence(acceptance.report['compaction_evidence']), recorded=True)
        except (KeyError, ValueError) as error:
            acceptance.report['error'] = str(error)
    acceptance.report['helper_routing'] = helper_verdict(acceptance.report.get('compaction_evidence'))
    if args.suite in ('full', 'helper') and acceptance.report['helper_routing']['verdict'] != 'VERIFIED':
        acceptance.report['missing'].append(HELPER_GATE)
    acceptance.report['p03_passed'] = 'p03_closed_client_live_inference_exact_approval_and_one_effect' in verified
    clean = not acceptance.report.get('cleanup_errors') and not acceptance.report.get('native_cleanup_error')
    acceptance.report['passed'] = not acceptance.report['missing'] and clean and not acceptance.report.get('error')
    acceptance.report['offline_passed'] = bool(args.recorded and acceptance.report.get('compaction_probe') and clean
        and acceptance.report.get('reconciled_budget') and acceptance.report.get('recorded_helper') and not acceptance.report.get('error'))
    acceptance.save()
    acceptance.write_evidence()
    print(json.dumps({'passed': acceptance.report['passed'], 'p03_passed': acceptance.report['p03_passed'],
        'clean': clean, 'missing': acceptance.report['missing']}))
    return 0 if clean and (acceptance.report['passed'] or acceptance.report['offline_passed'] or
        (args.suite == 'p03' and not acceptance.report.get('error'))) else 1


if __name__ == '__main__':
    raise SystemExit(main())
