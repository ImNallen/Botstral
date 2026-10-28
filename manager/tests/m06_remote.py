#!/usr/bin/env python3
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import m05_remote


def dispatch(root, p):
    op = p['op']
    if op == 'quiescent':
        manager_pid = int(m05_remote.run(['systemctl', 'show', f'user@{os.getuid()}.service', '--property=MainPID', '--value']).strip())
        assert manager_pid > 0
        manager_cgroup = Path(f'/proc/{manager_pid}/cgroup').read_text()
        ancestors = {os.getpid(), manager_pid}
        parent = os.getppid()
        while parent and parent not in ancestors:
            ancestors.add(parent)
            parent = int(Path(f'/proc/{parent}/stat').read_text().rsplit(')', 1)[1].split()[1])
        busy = set()
        for process in Path('/proc').iterdir():
            if not process.name.isdecimal() or int(process.name) in ancestors:
                continue
            try:
                if process.stat().st_uid != os.getuid():
                    continue
                if (process/'comm').read_text().strip() == '(sd-pam)':
                    parent_pid = int((process/'stat').read_text().rsplit(')', 1)[1].split()[1])
                    if parent_pid == manager_pid and (process/'cgroup').read_text() == manager_cgroup:
                        continue
                for link in (process/'cwd', process/'exe', *list((process/'fd').iterdir())):
                    try:
                        target = Path(os.readlink(link).removesuffix(' (deleted)'))
                        if target.is_relative_to(root):
                            busy.add(int(process.name))
                    except FileNotFoundError:
                        pass
            except (FileNotFoundError, ProcessLookupError):
                pass
        return sorted(busy)
    if op == 'start':
        if not p['unit'].startswith('botstral-m06-'):
            raise ValueError('fixture unit name')
        args = ['systemd-run', '--user', '--unit='+p['unit'], '--property=Type=exec',
                '--property=RemainAfterExit=yes', '--property=UMask=0077',
                '--property=WorkingDirectory='+str(root), '--property=StandardOutput=append:'+str(root/p['log']),
                '--property=StandardError=append:'+str(root/p['log'])]
        args += ['--setenv='+k+'='+v for k, v in p.get('environment', {}).items()]
        m05_remote.run([*args, p['executable'], *p['args']])
        return True
    if op == 'run':
        value = subprocess.run(p['args'], cwd=root, capture_output=True, timeout=p.get('timeout', 120))
        return {'code': value.returncode, 'stdout': value.stdout.decode(), 'stderr': value.stderr.decode()}
    if op == 'unlink':
        for name in p['paths']:
            (root/name).unlink(missing_ok=True)
        return True
    if op == 'rename':
        (root/p['source']).rename(root/p['destination'])
        return True
    if op == 'artifact':
        import http.client
        host, port = p['address'].rsplit(':', 1)
        assert host == '127.0.0.1'
        connection = http.client.HTTPConnection(host, int(port), timeout=15)
        connection.request('GET', p['path'], headers={'Authorization': 'Bearer '+p['token']})
        response = connection.getresponse()
        data = response.read()
        connection.close()
        return {'status': response.status, 'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data), 'base64': base64.b64encode(data).decode()}
    if op == 'gateway':
        import http.client
        import ssl
        from urllib.parse import urlparse
        config = json.loads((root/'agent/config.json').read_text())
        endpoint = urlparse(config['manager'])
        context = ssl.create_default_context(cafile=config['manager_ca'])
        context.load_cert_chain(config['client_cert'], config['client_key'])
        connection = http.client.HTTPSConnection(endpoint.hostname, endpoint.port, context=context, timeout=20)
        connection.request('POST', '/worker/v1/connectors/'+p['task']+'/'+p['execution'], json.dumps(p['body']), {'Content-Type': 'application/json'})
        response = connection.getresponse()
        data = response.read()
        connection.close()
        return {'status': response.status, 'body': json.loads(data) if data else None}
    if op == 'vibe_policy':
        import tomllib
        home = root/'agent/coding'/p['workspace']
        profile = tomllib.loads((home/'agents/managed.toml').read_text())
        config = tomllib.loads((home/'config.toml').read_text())
        return {'enabled_tools': profile['enabled_tools'], 'profile_bypass': profile['bypass_tool_permissions'],
                'config_bypass': config['bypass_tool_permissions'], 'tools': profile['tools'],
                'mcp_servers': [{'name': v['name'], 'transport': v['transport'], 'sampling_enabled': v['sampling_enabled']} for v in config['mcp_servers']]}
    if op == 'journal':
        import sqlite3
        with sqlite3.connect(root/'agent/agent.sqlite3') as db:
            return {'incarnation': db.execute('SELECT incarnation FROM metadata').fetchone()[0],
                    'executions': [{'task': value['body']['task'], 'execution': value['body']['execution'], 'work': value['body']['work']['type']}
                                   for (record,) in db.execute('SELECT record FROM executions') for value in [json.loads(record)]]}
    if op == 'https_auth':
        import http.client
        import ssl
        context = ssl.create_default_context(cafile=str(root/'ca.pem'))
        result = []
        for authorization in (None, 'Bearer synthetic-wrong-token'):
            connection = http.client.HTTPSConnection('192.168.1.40', p['port'], context=context, timeout=10)
            headers = {'Content-Type': 'application/json'}
            if authorization:
                headers['Authorization'] = authorization
            connection.request('POST', '/mcp', '{}', headers)
            response = connection.getresponse(); response.read(); connection.close()
            result.append(response.status)
        return result
    if op == 'scope':
        ready = json.loads((root/'agent/processes'/p['execution']/'ready.json').read_text())
        group = Path('/sys/fs/cgroup')/ready['scope']['cgroup'].lstrip('/')
        pids = []
        if group.exists():
            for file in group.rglob('cgroup.procs'):
                pids += [int(pid) for pid in file.read_text().split()]
        return {'ready': ready, 'pids': pids, 'proc_cgroups': {str(pid): Path(f'/proc/{pid}/cgroup').read_text() for pid in pids if Path(f'/proc/{pid}/cgroup').exists()},
                'commands': {str(pid): Path(f'/proc/{pid}/cmdline').read_bytes().replace(bytes([0]), b' ').decode() for pid in pids if Path(f'/proc/{pid}/cmdline').exists()},
                'survivors': [pid for pid in p.get('observed', []) if Path(f'/proc/{pid}').exists()]}
    if op == 'runtime':
        python = Path('/usr/bin/python3').resolve()
        digest = hashlib.sha256(python.read_bytes()).hexdigest()
        helper = (root/'src/agent/connector/runtime.py').read_bytes()
        runtime = hashlib.sha256(helper+digest.encode()).hexdigest()
        account = {'uid': os.getuid(), 'gid': os.getgid()}
        limits = {'runtime_seconds': 180, 'output_bytes': 1024, 'processes': 32, 'memory_bytes': 268435456}
        for name, config in [('process', {'profile': 'linux_systemd_v1', 'account': account, 'ceilings': limits}),
                             ('connector', {'python': str(python), 'python_sha256': digest, 'runtime': runtime, 'account': account, 'ceilings': limits})]:
            file = root/'agent'/(name+'.json')
            file.write_text(json.dumps(config)); file.chmod(0o600)
        manifest = root/'vibe-manifest.json'
        m05_remote.run(['/usr/bin/python3', str(root/'src/agent/vibe/seal.py'), '--runtime', p['vibe'], '--manifest', str(manifest)], timeout=120)
        vibe_hash = hashlib.sha256(manifest.read_bytes()).hexdigest()
        file = root/'agent/vibe.json'
        file.write_text(json.dumps({'directory': p['vibe'], 'manifest': str(manifest), 'runtime': vibe_hash, 'account': account, 'ceilings': limits})); file.chmod(0o600)
        return {'vibe_version': json.loads(manifest.read_text())['distributions']['mistral-vibe'], 'runner': {'runtime': runtime, 'account': account, 'limits': limits}, 'vibe': vibe_hash,
                'python_sha256': digest, 'vibe_binary_sha256': hashlib.sha256(Path(p['vibe']+'/bin/vibe').read_bytes()).hexdigest()}
    return m05_remote.dispatch(root, p)


if __name__ == '__main__':
    os.umask(0o077)
    root = Path(__file__).resolve().parent
    try:
        print(json.dumps({'ok': True, 'value': dispatch(root, json.load(sys.stdin))}))
    except Exception as error:
        print(json.dumps({'ok': False, 'error': type(error).__name__ + ' ' + str(error)}))
        sys.exit(1)
