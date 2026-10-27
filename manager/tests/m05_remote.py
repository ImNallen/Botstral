#!/usr/bin/env python3
import base64
import hashlib
import http.client
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import sys
import time


def run(args, timeout=15):
    result = subprocess.run(args, capture_output=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError('fixture subprocess failed')
    return result.stdout.decode()


def dispatch(root, p):
    def path(name):
        value = root/name
        if not value.resolve().is_relative_to(root.resolve()):
            raise ValueError('fixture path escaped')
        return value
    op = p['op']
    if op == 'write':
        for name, value in p['files'].items():
            file = path(name)
            file.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            file.write_bytes(base64.b64decode(value))
            file.chmod(0o700 if name.startswith('bin/') else 0o600)
        return True
    if op == 'read':
        file = path(p['path'])
        return file.read_text() if file.exists() else None
    if op == 'start':
        unit = p['unit']
        if not unit.startswith('botstral-m05-'):
            raise ValueError('fixture unit name')
        executable = path(p['executable'])
        log = path(p['log'])
        run(['systemd-run', '--user', '--unit='+unit, '--property=Type=exec', '--property=RemainAfterExit=yes', '--property=UMask=0077', '--property=WorkingDirectory='+str(root), '--property=StandardOutput=append:'+str(log), '--property=StandardError=append:'+str(log), str(executable), *p['args']])
        return True
    if op == 'unit':
        raw = run(['systemctl','--user','show',p['unit'],'--property=ActiveState,SubState,ExecMainPID,ExecMainStatus,ExecMainCode,Result'])
        return dict(line.split('=',1) for line in raw.splitlines())
    if op == 'stop':
        before = dispatch(root,dict(op='unit',unit=p['unit']))
        if before['ActiveState'] == 'active' and before['SubState'] == 'running':
            run(['systemctl','--user','kill','--signal=SIGTERM','--kill-whom=main',p['unit']])
        deadline=time.monotonic()+10
        while time.monotonic()<deadline:
            after=dispatch(root,dict(op='unit',unit=p['unit']))
            if after['SubState']!='running':
                break
            time.sleep(.05)
        run(['systemctl','--user','stop',p['unit']])
        after_stop=dispatch(root,dict(op='unit',unit=p['unit']))
        reset=False
        if after_stop['ActiveState']=='failed':
            run(['systemctl','--user','reset-failed',p['unit']])
            reset=True
        inactive=subprocess.run(['systemctl','--user','is-active',p['unit']],capture_output=True,text=True)
        if inactive.stdout.strip() not in ('inactive','unknown'):
            raise RuntimeError('fixture unit did not unload')
        pid=int(before.get('ExecMainPID','0'))
        alive=False
        if pid:
            try:
                os.kill(pid,0)
                alive=True
            except ProcessLookupError:
                pass
        if alive or after_stop['ActiveState']=='active':
            raise RuntimeError('fixture process still alive')
        return dict(before=before,exited=after,stopped=after_stop,pid_absent=not alive,reset_failed=reset,final_unit_state=inactive.stdout.strip())
    if op == 'http':
        host,port=p['address'].rsplit(':',1)
        if host!='127.0.0.1':
            raise ValueError('bootstrap HTTP must remain loopback')
        connection=http.client.HTTPConnection(host,int(port),timeout=10)
        headers={'Content-Type':'application/json','Connection':'close'}
        if p.get('token'):
            headers['Authorization']='Bearer '+p['token']
        connection.request(p['method'],p['path'],json.dumps(p.get('body',{})),headers)
        response=connection.getresponse()
        data=response.read()
        connection.close()
        return dict(status=response.status,body=json.loads(data) if data else None)
    if op == 'http_batch':
        return [dispatch(root,dict(op='http',**item)) for item in p['requests']]
    if op == 'native':
        args=[str(path('bin/owner-probe')),p['command'],str(path('client')),p['value']]
        result=subprocess.run(args,capture_output=True,timeout=130)
        if p['command']=='call' and result.returncode==0:
            value=json.loads(result.stdout)
            summary={'type':value['type']}
            if value['type']=='replay':
                replay=value['value']
                summary.update(cursor=replay['cursor'],replay=replay['type'])
                if replay['type']=='snapshot':
                    summary['collections']={k:len(v) for k,v in replay['state'].items()}
            return dict(code=0,summary=summary,bytes=len(result.stdout))
        return dict(code=result.returncode,stdout=result.stdout.decode(),stderr=result.stderr.decode())
    if op == 'worker_enroll':
        return run([str(path('bin/botstral-agent')),'enroll','--state-dir',str(path('agent')),'--manager','wss://'+p['address'],'--manager-ca',str(path('ca.pem')),'--client-cert',str(path('worker.pem')),'--client-key',str(path('worker.key')),'--invitation-file',str(path('worker-invitation.json'))],timeout=20).strip()
    if op == 'state':
        file=path('client/state.json')
        if not file.exists():
            return None
        state=json.loads(file.read_text())
        return dict(manager=state['manager'],cursor=state['cursor'],collections={k:len(v) for k,v in state['state'].items()})
    if op == 'leaf':
        host,port=p['address'].rsplit(':',1)
        context=ssl.create_default_context(cafile=str(path('ca.pem')))
        with socket.create_connection((host,int(port)),timeout=10) as raw:
            with context.wrap_socket(raw,server_hostname=p['name']) as tls:
                return hashlib.sha256(tls.getpeercert(binary_form=True)).hexdigest()
    if op == 'hashes':
        return {name:hashlib.sha256(path(name).read_bytes()).hexdigest() for name in p['files']}
    if op == 'remove_listeners':
        path('state/listeners.json').unlink(missing_ok=True)
        return True
    if op == 'bore_version':
        return run([str(path('bin/bore')),'--version']).strip()
    if op == 'cleanup':
        shutil.rmtree(root)
        return not root.exists()
    raise ValueError('unknown fixture operation')


if __name__=='__main__':
    os.umask(0o077)
    payload=json.load(sys.stdin)
    root=Path(__file__).resolve().parent
    try:
        result=dispatch(root,payload)
        print(json.dumps({'ok':True,'value':result}))
    except Exception as error:
        print(json.dumps({'ok':False,'error':type(error).__name__}))
        sys.exit(1)
