"""Install actual Debian packages and reopen actual application state on a disposable host."""
from http.client import HTTPConnection
import json
import os
from pathlib import Path
import platform
import pwd
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace

from release_acceptance import ROOT, run, refused
from release_artifacts import inspect_deb
from release_model import authority, digest, require


def fixture_binary():
    result = subprocess.check_output(['cargo', 'test', '-p', 'botstral-manager', '--locked', '--lib', '--no-run', '--message-format=json'], cwd=ROOT, text=True)
    outputs = [json.loads(line) for line in result.splitlines() if line.startswith('{')]
    return Path(next(v['executable'] for v in outputs if v.get('reason') == 'compiler-artifact' and v.get('executable') and v['target']['name'] == 'botstral_manager'))


def snapshot(state):
    with sqlite3.connect(state / 'manager.sqlite3') as db:
        tables = ['manager_identity', 'owner_identity', 'vault', 'provider_keys', 'messages', 'artifacts', 'workers']
        values = {name: db.execute('SELECT * FROM ' + name).fetchall() for name in tables}
        values['attention'] = [row for row in db.execute('SELECT view FROM tasks') if 'awaiting_attention' in row[0]]
    values['blobs'] = {p.name: digest(p) for p in (state / 'artifacts').iterdir()}
    return values


def manager_start(binary, state, demote):
    (state / 'listeners.json').unlink(missing_ok=True)
    log = (state.parent / 'manager.log').open('ab')
    process = subprocess.Popen([str(binary), '--state-dir', str(state), '--listen', '127.0.0.1:0', '--setup-listen', '127.0.0.1:0'], preexec_fn=demote, stdout=log, stderr=log)
    log.close()
    until = time.monotonic() + 20
    while not (state / 'listeners.json').exists():
        require(process.poll() is None, 'packaged manager exited; inspect manager.log')
        require(time.monotonic() < until, 'packaged manager startup deadline')
        time.sleep(.02)
    return process


def locked_and_unlock(state):
    address = json.loads((state / 'listeners.json').read_text())['listen']
    host, port = address.rsplit(':', 1)
    token = None
    def request(method, path, value=None):
        connection = HTTPConnection(host, int(port), timeout=15)
        headers = {'Content-Type': 'application/json', 'Connection': 'close'}
        if token:
            headers['Authorization'] = 'Bearer ' + token
        try:
            connection.request(method, path, json.dumps(value or {}), headers)
            response = connection.getresponse()
            body = response.read()
            return response.status, json.loads(body) if body else None
        finally:
            connection.close()
    status, session = request('POST', '/v1/sessions', {'password': 'synthetic recovery login password'})
    require(status == 201, 'packaged manager could not reopen owner login')
    token = session['token']
    status, value = request('GET', '/v1/status')
    require(status == 200 and value['vault'] == 'locked' and value['provider'] is not None, 'packaged manager did not restart locked with retained credential metadata')
    status, _ = request('POST', '/v1/vault/unlock', {'passphrase': 'synthetic recovery vault passphrase'})
    require(status == 204, 'explicit vault unlock failed through packaged executable')
    status, value = request('GET', '/v1/status')
    require(status == 200 and value['vault'] == 'unlocked', 'packaged manager remained locked after explicit unlock')
    status, bots = request('GET', '/v1/bots')
    require(status == 200 and bots, 'retained bot missing')
    bot = bots[0]['id']
    status, current = request('GET', '/v1/inference-configurations/' + bot)
    revision = current['revision'] if status == 200 else 0
    now = int(time.time())
    policy = {'endpoint': 'global', 'model_id': 'mistral-small-2506', 'context_tokens': 32768,
              'acknowledgment': {'model_developer': 'Mistral AI', 'developer_terms_url': 'https://mistral.ai/terms/',
                                 'privacy_terms_url': 'https://mistral.ai/privacy/', 'acknowledged_at': now, 'release': {'type': 'stable'}},
              'prices': {'input_microusd_per_million_tokens': 1000000, 'output_microusd_per_million_tokens': 2000000,
                         'effective_at': now - 1, 'expires_at': now + 86400, 'billing': 'text_tokens_only_at_acknowledged_ceilings'}}
    status, configuration = request('PUT', '/v1/inference-configurations', {'bot': bot, 'expected_revision': revision, 'policy': policy})
    require(status == 200, 'packaged manager could not configure recorded provider')
    status, verified = request('POST', '/v1/inference-configurations/verify', {'bot': bot, 'revision': configuration['revision']})
    require(status == 200 and verified['verification']['type'] == 'verified', 'packaged manager did not use retained credential against recorded HTTPS provider: ' + str((status, verified)))



def stop(process):
    process.send_signal(signal.SIGTERM)
    require(process.wait(timeout=20) == 0, 'packaged manager shutdown failed')


def manager_accept(old_package, new_package, other_package, helper, root, demote, environment, production):
    unmanaged = Path('/usr/local/bin/botstral-manager')
    run(['dpkg-deb', '--extract', old_package, root / 'unmanaged-stage'])
    shutil.copyfile(root / 'unmanaged-stage/usr/bin/botstral-manager', unmanaged)
    try:
        refused(['dpkg', '--install', old_package], 'unmanaged installation')
    finally:
        unmanaged.unlink()
    unmanaged = Path('/usr/bin/botstral-manager')
    shutil.copyfile(root / 'unmanaged-stage/usr/bin/botstral-manager', unmanaged)
    try:
        refused(['dpkg', '--install', old_package], 'existing unmanaged path')
    finally:
        unmanaged.unlink()
        shutil.rmtree(root / 'unmanaged-stage')
    run(['dpkg', '--install', old_package])
    refused(['dpkg', '--install', other_package], 'conflicts with botstral-manager')
    binary = Path('/usr/bin/botstral-manager')
    Path('/etc/botstral-manager').mkdir(exist_ok=True, mode=0o700)
    canary = Path('/etc/botstral-manager/service.json')
    canary.write_text('preserved acceptance configuration\n')
    canary.chmod(0o600)
    run([helper, 'release_seed_fixture', '--ignored', '--nocapture'], preexec_fn=demote, env=environment)
    state = root / 'source'
    before = snapshot(state)
    require(before['attention'] and before['messages'] and before['blobs'] and before['workers'], 'fixture did not seed retained task/history/artifact/worker evidence')
    process = manager_start(binary, state, demote)
    try:
        refused(['dpkg', '--install', new_package], 'component is active')
        refused(['/usr/bin/botstral-manager-backup', 'create', '--state-dir', state, '--destination', root / 'active-backup'], 'Resource temporarily unavailable', preexec_fn=demote)
    finally:
        stop(process)
    before = snapshot(state)
    backup = root / 'offline-backup'
    run(['/usr/bin/botstral-manager-backup', 'create', '--state-dir', state, '--destination', backup], preexec_fn=demote)
    run(['/usr/bin/botstral-manager-backup', 'verify', '--backup', backup], preexec_fn=demote)
    run(['dpkg', '--install', new_package])
    require(canary.read_text() == 'preserved acceptance configuration\n', 'live configuration changed')
    installed = json.loads(subprocess.check_output([str(binary), '--version-json']))
    process = manager_start(binary, state, demote)
    try:
        locked_and_unlock(state)
    finally:
        stop(process)
    require(snapshot(state) == before, 'manager identity, credentials, history, worker pairing, artifacts, or uncertainty changed')
    run([helper, 'release_verify_fixture', '--ignored', '--nocapture'], preexec_fn=demote, env=environment)
    refused(['dpkg', '--install', old_package], 'downgrade requires full backup recovery')
    # A failed target startup must leave state available. A future schema is a deliberate failure control.
    with sqlite3.connect(state / 'manager.sqlite3') as db:
        db.execute('PRAGMA user_version=999')
    failed = subprocess.run([str(binary), '--state-dir', str(state), '--listen', '127.0.0.1:0'], preexec_fn=demote, timeout=20, capture_output=True)
    require(failed.returncode != 0, 'future schema startup was not refused')
    run(['dpkg', '--remove', 'botstral-manager'])
    state.rename(root / 'failed-target-state')
    run(['dpkg', '--install', old_package])
    run(['/usr/bin/botstral-manager-backup', 'restore', '--backup', backup, '--destination', state], preexec_fn=demote)
    process = manager_start(binary, state, demote)
    try:
        locked_and_unlock(state)
    finally:
        stop(process)
    require(snapshot(state) == before, 'backup-based recovery differs from offline state')
    run([helper, 'release_verify_fixture', '--ignored', '--nocapture'], preexec_fn=demote, env=environment)
    run(['dpkg', '--remove', 'botstral-manager'])
    return installed


def agent_accept(old_package, new_package, root, demote, environment, production, package_only):
    run(['dpkg', '--install', old_package])
    # Reuse the existing real production-journal recovery fixture with selected runtime roots.
    sys.path.insert(0, str(ROOT / 'agent/packaging'))
    import importlib.util
    spec = importlib.util.spec_from_file_location('worker_container_acceptance', ROOT / 'agent/packaging/container-acceptance.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    connector = ROOT / 'agent/connector'
    files = {name: (connector / name).read_text() for name in ('configure.py', 'runtime.py')}
    source = 'CONNECTOR_FILES = ' + repr(files) + '\n' + module.FIXTURE
    source = source.replace("pathlib.Path('/home/botstral-agent')", 'pathlib.Path(' + repr(str(root)) + ')')
    # Leave the restored production journal in place for the new binary to reopen.
    if package_only:
        source = source.split("plan = home / 'fixture-plan.json'", 1)[0]
    script = root / 'worker-fixture.py'
    script.write_text(source)
    script.chmod(0o644)
    run([sys.executable, script], preexec_fn=demote, env=environment)
    state = root / 'fixture-state'
    with sqlite3.connect(state / 'agent.sqlite3') as db:
        previous = db.execute('SELECT incarnation, head, acknowledged FROM metadata').fetchall()
    retained = {str(p.relative_to(root)): digest(p) for p in root.rglob('*') if p.is_file() and p.suffix != '.sqlite3' and not p.name.endswith(('-wal', '-shm'))}
    run(['dpkg', '--install', new_package])
    check = subprocess.run(['/usr/bin/botstral-agent', 'check', '--state-dir', str(state)], preexec_fn=demote, env=environment, capture_output=True, timeout=30)
    require(check.returncode == 1, 'invalid fixture TLS unexpectedly connected')
    with sqlite3.connect(state / 'agent.sqlite3') as db:
        require(db.execute('SELECT incarnation, head, acknowledged FROM metadata').fetchall() == previous, 'worker journal identity changed')
    require(all(digest(root / name) == sha for name, sha in retained.items()), 'worker configuration, certificates or runtime changed')
    refused(['dpkg', '--install', old_package], 'downgrade requires full backup recovery')
    # Interrupt package payload integrity and reinstall the same exact native artifact.
    Path('/usr/bin/botstral-agent').write_bytes(b'interrupted native payload')
    run(['dpkg', '--install', new_package])
    require(json.loads(subprocess.check_output(['/usr/bin/botstral-agent', '--version-json']))['identity']['version'] in new_package.name, 'repair did not restore exact executable')
    if production:
        run(['/usr/bin/botstral-agent-supervisor-check', '--scratch', root / 'supervisor-proof'], preexec_fn=demote, env=environment)
    run(['dpkg', '--remove', 'botstral-agent'])


def accept(args, old, new, old_artifacts, artifacts):
    require(sys.platform == 'linux' and platform.machine() == 'x86_64' and os.geteuid() == 0, 'root on disposable Ubuntu amd64 required')
    require(os.environ.get('BOTSTRAL_DISPOSABLE_RELEASE_HOST') == '1', 'set BOTSTRAL_DISPOSABLE_RELEASE_HOST=1 on an isolated disposable host only')
    require('VERSION_ID="24.04"' in Path('/etc/os-release').read_text(), 'Ubuntu 24.04 acceptance required')
    if args.production:
        require(subprocess.check_output(['systemctl', '--version'], text=True).startswith('systemd 255'), 'systemd 255 required')
    for package in ('botstral-manager', 'botstral-agent'):
        require(not Path('/usr/bin/' + package).exists() and not Path('/usr/lib/' + package).exists(), 'acceptance host already has Botstral installed')
    helper = fixture_binary()
    account = 'botstral-release-test'
    run(['useradd', '--create-home', '--shell', '/bin/bash', account])
    user = pwd.getpwnam(account)
    def demote():
        os.setgroups([])
        os.setgid(user.pw_gid)
        os.setuid(user.pw_uid)
    environment = dict(os.environ, HOME=user.pw_dir, XDG_RUNTIME_DIR='/run/user/' + str(user.pw_uid), DBUS_SESSION_BUS_ADDRESS='unix:path=/run/user/' + str(user.pw_uid) + '/bus')
    if args.production:
        run(['loginctl', 'enable-linger', account])
        run(['systemctl', 'start', 'user@' + str(user.pw_uid) + '.service'])
    with tempfile.TemporaryDirectory(prefix='botstral-native-') as directory:
        root = Path(directory)
        root.chmod(0o755)
        for component in ('manager', 'agent'):
            previous = next(a for a in old_artifacts if a.component == component)
            artifact = next(a for a in artifacts if a.component == component)
            old_package = args.old.resolve() / previous.filename
            new_package = args.new.resolve() / artifact.filename
            for path, metadata, identity in [(old_package, previous, old), (new_package, artifact, new)]:
                release = SimpleNamespace(identity=identity, value=dict(authority(), link_schema_sha256=digest(ROOT / 'link/schema/protocol.json')))
                inspect_deb(path, metadata, release)
            require(subprocess.run(['dpkg', '--compare-versions', previous.native_versions['debian'], 'lt', artifact.native_versions['debian']]).returncode == 0, 'native versions do not sort correctly')
            fixture = root / component
            fixture.mkdir(mode=0o700)
            os.chown(fixture, user.pw_uid, user.pw_gid)
            if component == 'manager':
                environment['BOTSTRAL_RELEASE_FIXTURE'] = str(fixture)
                from release_acceptance_provider import provider
                with provider(fixture) as observed:
                    other_package = args.new.resolve() / next(a.filename for a in artifacts if a.component == 'agent')
                    actual = manager_accept(old_package, new_package, other_package, helper, fixture, demote, environment, args.production)
                    require(len(observed) >= 2 and all(observed), 'packaged credential use was not observed before and after recovery')
                require(actual['identity'] == new.json(), 'installed manager identity mismatch')
            else:
                agent_accept(old_package, new_package, fixture, demote, environment, args.production, args.package_only)
    return {'real_debian_old_to_new': True, 'manager_locked_restart_and_credential_decryption_after_unlock': True,
            'manager_identity_worker_pairing_history_artifacts_and_attention_retained': True,
            'worker_journal_certificates_and_runtime_retained': True,
            'active_replacement_and_downgrade_refused': True, 'future_schema_failure_and_backup_recovery': True,
            'native_package_repair': True, 'production_systemd_supervisor': args.production,
            'worker_backup_and_runtime_diagnostics': not args.package_only,
            'scope': 'independent component hosts simulated sequentially; fixture worker pairing is synthetic; no external runtime execution claimed'}
