#!/usr/bin/env python3
"""Verify pinned LAN and temporary bore traffic between isolated real-binary fixtures."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import socket
import ssl
import subprocess
import tempfile
import time
import uuid
import m05_acceptance as local


class Remote:
    def __init__(self,alias,root):
        self.alias,self.root=alias,root

    def ssh(self,args,payload=None,timeout=150):
        result=subprocess.run(['ssh','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes',self.alias,shlex.join(args)],input=json.dumps(payload) if payload is not None else None,text=True,capture_output=True,timeout=timeout)
        local.require(result.returncode==0,'fixture SSH helper failed on '+self.alias)
        return json.loads(result.stdout)

    def install(self):
        source=Path(__file__).with_name('m05_remote.py').read_text()
        bootstrap="import os,pathlib,json,sys;os.umask(0o077);p=json.load(sys.stdin);r=pathlib.Path(p['root']);r.mkdir(mode=0o700);(r/'bin').mkdir(mode=0o700);(r/'m05_remote.py').write_text(p['source']);print(json.dumps(True))"
        self.ssh(['python3','-c',bootstrap],{'root':self.root,'source':source})

    def call(self,op,**values):
        result=self.ssh(['python3',self.root+'/m05_remote.py'],dict(op=op,**values))
        local.require(result.get('ok'),'remote '+op+' failed on '+self.alias+' '+result.get('error',''))
        return result['value']

    def files(self,files):
        return self.call('write',files={k:base64.b64encode(v).decode() for k,v in files.items()})

    def binary(self,source,name):
        subprocess.run(['scp','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes',str(source),self.alias+':'+self.root+'/bin/'+name],check=True,capture_output=True)


def poll(read,accept,label,seconds=15):
    until=time.monotonic()+seconds
    while time.monotonic()<until:
        value=read()
        if accept(value):
            return value
        time.sleep(.1)
    raise TimeoutError(label)


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--manager-alias',default='botstral-manager')
    parser.add_argument('--computer-alias',default='botstral-computer')
    parser.add_argument('--manager-address',default='192.168.1.40')
    parser.add_argument('--binaries',type=Path,required=True)
    parser.add_argument('--legacy-agent-bin',type=Path)
    parser.add_argument('--bore-bin',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    os.umask(0o077)
    args.output.mkdir(mode=0o700,parents=True,exist_ok=True)
    identifier=uuid.uuid4().hex[:12]
    root='/home/botstral/m05-fixture-'+identifier
    manager,computer=Remote(args.manager_alias,root),Remote(args.computer_alias,root)
    certs=Path(tempfile.mkdtemp(prefix='m05-bootstrap-',dir=args.output))
    units=[]
    evidence={'checks':[],'cleanup':[],'root':root,'source_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()}
    def passed(name,**details):
        evidence['checks'].append(dict(name=name,**details))
        print('PASS '+name,flush=True)
    def start(remote,name,executable,arguments):
        unit='botstral-m05-'+identifier+'-'+name
        remote.call('start',unit=unit,executable='bin/'+executable,args=arguments,log=name+'.log')
        units.append((remote,unit,name+'.log'))
        return unit
    def stop(remote,unit):
        entry=next((entry for entry in units if entry[1]==unit),None)
        if entry:
            content=remote.call('read',path=entry[2])
            if content is not None:
                (args.output/(unit+'.log')).write_text(content)
        result=remote.call('stop',unit=unit)
        evidence['cleanup'].append(dict(host=remote.alias,unit=unit,**result))
        units[:]=[entry for entry in units if entry[1]!=unit]
        return result
    token=None
    addresses=None
    def owner(method,path,body=None,status=200):
        response=manager.call('http',address=addresses['listen'],method=method,path=path,body=body or {},token=token)
        local.require(response['status']==status,'fixture owner HTTP status for '+path)
        return response['body']
    def manager_start(name):
        manager.call('remove_listeners')
        arguments=['--state-dir',root+'/state','--listen',addresses['listen'] if addresses else '127.0.0.1:0','--setup-listen','127.0.0.1:0','--public-listen',addresses['public'] if addresses else '0.0.0.0:0','--public-tls-cert',root+'/manager.pem','--public-tls-key',root+'/manager.key','--public-worker-ca',root+'/ca.pem']
        unit=start(manager,name,'botstral-manager',arguments)
        value=poll(lambda:manager.call('read',path='state/listeners.json'),lambda v:v is not None,'fixture Manager readiness')
        return unit,json.loads(value)
    try:
        local.certificates(certs)
        for remote in (manager,computer):
            remote.install()
            remote.files({'ca.pem':(certs/'ca.pem').read_bytes()})
        manager.files({name:(certs/name).read_bytes() for name in ('manager.pem','manager.key')})
        computer.files({name:(certs/name).read_bytes() for name in ('worker.pem','worker.key')})
        manager.binary(args.binaries/'botstral-manager','botstral-manager')
        manager.binary(args.bore_bin,'bore')
        computer.binary(args.legacy_agent_bin or args.binaries/'botstral-agent','botstral-agent')
        computer.binary(args.binaries/'owner-probe','owner-probe')
        hashes=manager.call('hashes',files=['bin/botstral-manager','bin/bore'])|computer.call('hashes',files=['bin/botstral-agent','bin/owner-probe'])
        for name,digest in hashes.items():
            source=args.bore_bin if name=='bin/bore' else args.legacy_agent_bin if name=='bin/botstral-agent' and args.legacy_agent_bin else args.binaries/name.split('/')[-1]
            local.require(hashlib.sha256(source.read_bytes()).hexdigest()==digest,'deployed binary hash mismatch')
        evidence['binary_sha256']=hashes
        evidence['legacy_worker']=args.legacy_agent_bin is not None
        evidence['bore_version']=manager.call('bore_version')
        local.require(evidence['bore_version']=='bore-cli 0.6.0','unexpected tunnel version')
        manager_unit,addresses=manager_start('manager')
        proof=manager.call('read',path='state/bootstrap.token')
        response=manager.call('http',address=addresses['setup'],method='POST',path='/v1/owner',token=proof,body={'name':'Synthetic remote owner','password':'synthetic M05 remote password','vault_passphrase':'synthetic M05 remote vault passphrase','acknowledge_no_escrow':True})
        local.require(response['status']==201,'trusted local bootstrap failed')
        token=owner('POST','/v1/sessions',{'password':'synthetic M05 remote password'},status=201)['token']
        port=addresses['public'].rsplit(':',1)[1]
        direct=args.manager_address+':'+port
        trust=owner('POST','/v1/device-trust',{'endpoint':direct,'server_name':args.manager_address,'ca_pem':(certs/'ca.pem').read_text(),'certificate_pem':(certs/'manager.pem').read_text()})
        request=json.loads(computer.call('native',command='init',value='Synthetic remote client')['stdout'])
        grant=owner('POST','/v1/device-invitations',{'request':request,'lifetime_seconds':600})
        bundle={'trust':trust,**{key:grant[key] for key in ('role','fingerprint','expires_at','invitation')}}
        computer.files({'invitation.json':json.dumps(bundle).encode(),'resume.json':json.dumps({'method':'resume','cursor':0,'limit':64}).encode()})
        local.require(computer.call('native',command='import',value=root+'/invitation.json')['code']==0,'native import failed')
        first=computer.call('native',command='call',value=root+'/resume.json')
        local.require(first['code']==0 and first['summary']['replay']=='snapshot','LAN owner access failed')
        lan_leaf=computer.call('leaf',address=direct,name=args.manager_address)
        local.require(lan_leaf==trust['leaf'],'LAN manager leaf mismatch')
        passed('real_native_owner_over_direct_lan',endpoint=direct,leaf=lan_leaf,reply=first)
        fingerprint=hashlib.sha256(ssl.PEM_cert_to_DER_cert((certs/'worker.pem').read_text())).hexdigest()
        invitation=owner('POST','/v1/worker-invitations',{'fingerprint':fingerprint,'lifetime_seconds':600},status=201)
        computer.files({'worker-invitation.json':json.dumps(invitation).encode()})
        computer.call('worker_enroll',address=direct)
        worker_unit=start(computer,'worker','botstral-agent',['run','--state-dir',root+'/agent'])
        worker=poll(lambda:owner('GET','/v1/workers'),lambda v:len(v)==1 and v[0]['phase']=='ready','real worker readiness')[0]
        passed('unchanged_production_worker_over_same_public_listener',generation=worker['generation'])
        tunnel_unit=start(manager,'bore','bore',['local',port,'--to','bore.pub'])
        log=poll(lambda:manager.call('read',path='bore.log'),lambda v:v is not None and re.search(r'bore\.pub:(\d+)',v),'bore endpoint',seconds=25)
        relay='bore.pub:'+re.search(r'bore\.pub:(\d+)',log).group(1)
        relay_leaf=computer.call('leaf',address=relay,name='bore.pub')
        context=ssl.create_default_context(cafile=str(certs/'ca.pem'))
        host,relay_port=relay.rsplit(':',1)
        with socket.create_connection((host,int(relay_port)),timeout=10) as raw:
            with context.wrap_socket(raw,server_hostname='bore.pub') as connection:
                operator_leaf=hashlib.sha256(connection.getpeercert(binary_form=True)).hexdigest()
        local.require(lan_leaf==relay_leaf==operator_leaf,'relay terminated or replaced inner Manager TLS')
        for path in ('/v1/owner','/v1/sessions','/v1/device-invitations'):
            code,_=local.http(relay,'POST',path,{},context=context,headers={'Forwarded':'for=127.0.0.1','X-Forwarded-For':'127.0.0.1'})
            local.require(code==404,'local administration exposed through relay')
        passed('public_relay_excludes_setup_and_local_administration_with_forwarded_headers')
        relay_trust=dict(trust,endpoint=relay,server_name='bore.pub')
        computer.files({'relay-trust.json':json.dumps(relay_trust).encode()})
        local.require(computer.call('native',command='trust',value=root+'/relay-trust.json')['code']==0,'explicit relay profile import failed')
        external=computer.call('native',command='call',value=root+'/resume.json')
        local.require(external['code']==0,'public relay did not carry native protocol data')
        passed('actual_external_bore_data_path_with_unchanged_inner_tls',endpoint=relay,leaf=relay_leaf,operator_leaf=operator_leaf,reply=external)
        owner_unit=start(computer,'owner-watch','owner-probe',['watch',root+'/client','22'])
        initial=poll(lambda:computer.call('state'),lambda v:v is not None,'native initial application state')
        stop(manager,manager_unit)
        manager_unit,addresses=manager_start('manager-restarted')
        token=owner('POST','/v1/sessions',{'password':'synthetic M05 remote password'},status=201)['token']
        bot=str(uuid.uuid4())
        owner('PUT','/v1/bots/'+bot,{'name':'Remote recovery fixture'})
        conversation=str(uuid.uuid4())
        owner('POST','/v1/conversations',{'id':conversation,'bot':bot})
        results=manager.call('http_batch',requests=[dict(address=addresses['listen'],method='POST',path='/v1/messages',token=token,body={'id':str(uuid.uuid4()),'conversation':conversation,'text':'r'*4096}) for _ in range(160)])
        local.require(all(result['status']==200 for result in results),'fixture message population failed')
        restored=poll(lambda:computer.call('state'),lambda v:v is not None and v['collections']['messages']==160,'external reconnect snapshot',seconds=30)
        poll(lambda:computer.call('unit',unit=owner_unit),lambda v:v['SubState']=='exited','native watch completion',seconds=30)
        result=stop(computer,owner_unit)
        local.require(result['exited']['ExecMainStatus']=='0','native relay watch failed')
        local.require(restored['cursor']>initial['cursor'],'reconnect did not advance application state')
        passed('actual_native_reconnect_and_durable_recovery_through_public_relay',before=initial,after=restored)
        owner_unit=start(computer,'owner-revoke','owner-probe',['watch',root+'/client','60'])
        poll(lambda:computer.call('unit',unit=owner_unit),lambda v:v['SubState']=='running','active remote owner')
        time.sleep(1)
        device=owner('GET','/v1/devices')[0]['id']
        owner('DELETE','/v1/devices/'+device,status=204)
        poll(lambda:computer.call('unit',unit=owner_unit),lambda v:v['SubState']!='running','revocation closes current owner',seconds=10)
        result=stop(computer,owner_unit)
        future=computer.call('native',command='call',value=root+'/resume.json')
        local.require(future['code']!=0 and 'Authentication(Unauthorized)' in future['stderr'],'revoked owner reconnected through relay')
        local.require(owner('GET','/v1/workers')[0]['phase']=='ready','owner revocation stopped the worker')
        passed('current_and_future_remote_owner_revocation_with_worker_progress',owner_exit=result['exited']['ExecMainStatus'])
        stop(manager,manager_unit)
        manager_unit,addresses=manager_start('manager-revoked-restart')
        future=computer.call('native',command='call',value=root+'/resume.json')
        local.require(future['code']!=0 and 'Authentication(Unauthorized)' in future['stderr'],'revoked device returned after Manager restart')
        passed('remote_revocation_persists_after_restart')
        evidence['passed']=True
    finally:
        cleanup_errors=[]
        for remote,unit,logname in list(reversed(units)):
            try:
                stop(remote,unit)
            except Exception as error:
                cleanup_errors.append(type(error).__name__+' '+remote.alias+' '+unit)
        def refuses(address):
            host,port=address.rsplit(':',1)
            until=time.monotonic()+6
            while time.monotonic()<until:
                try:
                    with socket.create_connection((host,int(port)),timeout=1):
                        pass
                except OSError:
                    return True
                time.sleep(.1)
            return False
        processes_stopped=not cleanup_errors
        if 'relay' in locals() and 'direct' in locals():
            evidence['shutdown_endpoints']={'lan_refuses_tcp':refuses(direct),'relay_refuses_tcp':refuses(relay)}
            if not all(evidence['shutdown_endpoints'].values()):
                cleanup_errors.append('fixture endpoint still accepts TCP')
        if processes_stopped:
            for remote in (manager,computer):
                try:
                    evidence['cleanup'].append(dict(host=remote.alias,fixture_removed=remote.call('cleanup')))
                except Exception as error:
                    cleanup_errors.append(type(error).__name__+' '+remote.alias+' root')
        shutil.rmtree(certs)
        evidence['cleanup_errors']=cleanup_errors
        local.write(args.output/'summary.json',evidence)
        local.require(not cleanup_errors,'fixture cleanup failed')


if __name__=='__main__':
    main()
