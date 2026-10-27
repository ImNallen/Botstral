#!/usr/bin/env python3
"""Drive production Computer work, sealed artifacts and restart recovery on two VMs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time
import uuid
import m02_two_vm as m02
import w01_two_vm as process
from w01_runtime_measure import HTML

class Remote(process.Remote):
    def install(self):
        names=('m02_remote.py','w01_remote.py','w01_computer_remote.py')
        sources={n:Path(__file__).with_name(n).read_text() for n in names}
        bootstrap="import os,json,pathlib,sys; os.umask(0o077); p=json.load(sys.stdin); r=pathlib.Path(p['root']); r.mkdir(mode=0o700,parents=True,exist_ok=False); [(r/n).write_text(s) for n,s in p['sources'].items()]; print(json.dumps({'ok':True,'value':True}))"
        self.ssh(['python3','-c',bootstrap],{'root':self.root,'sources':sources})
    def call(self,op,**values):
        return self.ssh(['python3',self.root+'/w01_computer_remote.py'],{'op':op,**values})


def computer(f,session,operations,outputs=(),runtime_seconds=60,deadline_seconds=120):
    deadline=int(time.time())+deadline_seconds
    return {'id':str(uuid.uuid4()),'session_id':session['id'],'conversation_id':None,'settings':{'model':'unavailable','max_turns':1,'max_tokens':1,'deadline':deadline,'max_cost_micros':0},'work':{'type':'computer','spec':{'profile':'linux_playwright_v1','account':{'uid':1000,'gid':1000},'runtime':{'sha256':f.runtime},'deadline':deadline,'limits':{'runtime_seconds':runtime_seconds,'output_bytes':1024,'processes':128,'memory_bytes':1073741824},'viewport':{'width':800,'height':600},'operations':operations,'outputs':list(outputs)}}}

def output(slot,media='text_plain',maximum=4096):
    return {'slot':slot,'name':slot+('.png' if media=='png' else '.txt'),'media_type':media,'max_bytes':maximum}

def submit(f,request):
    original=f.owner('POST','/v1/tasks',request);f.owner('PUT','/v1/tasks/'+original['id']+'/approval',{'expires_at':min(int(time.time())+90,request['settings']['deadline'])});return original

def ready(f,original):
    return f.poll(lambda:f.owner('GET','/v1/tasks/'+original['id']),lambda t:all(o['state']['type'] in ('available','unavailable') for o in t['outputs']),'artifact availability')

def download(f,task,slot,expected=None):
    o=next(o for o in task['outputs'] if o['declaration']['slot']==slot)
    m02.require(o['state']['type']=='available','output unavailable '+slot)
    state=o['state'];path='/v1/tasks/'+task['id']+'/artifacts/'+state['id']
    result=f.manager.call('download',address=f.listen,path=path,token=f.owner_token)
    m02.require(result['status']==200 and result['length']==state['content']['length'] and result['sha256']==state['content']['sha256'],'download integrity changed')
    m02.require(result['headers']['content-disposition'].startswith('attachment;') and result['headers']['x-content-type-options']=='nosniff' and result['headers']['cache-control']=='no-store','unsafe download headers')
    if expected is not None:m02.require(result['sha256']==hashlib.sha256(expected).hexdigest() and result['length']==len(expected),'download bytes differ')
    return result,path

def exercise(f,certificates,args):
    with f.check('computer_enrollment_and_runtime_probe') as details:
        f.enroll(f.invitation(certificates),certificates)
        a=f.session('Computer A');b=f.session('Computer B')
        f.runtime=hashlib.sha256(Path(args.runtime_manifest).read_bytes()).hexdigest()
        f.worker.call('write',files={f.worker.root+'/agent/process.json':json.dumps({'profile':'linux_systemd_v1','account':{'uid':1000,'gid':1000},'ceilings':process.limits()}),f.worker.root+'/agent/computer.json':json.dumps({'account':{'uid':1000,'gid':1000},'runtime':{'sha256':f.runtime},'directory':args.runtime,'ceilings':{'runtime_seconds':60,'output_bytes':1024,'processes':128,'memory_bytes':1073741824}})})
        f.worker.call('write',files={f.worker.root+'/agent/computer-probe/incomplete':'unpublished probe fixture'})
        f.start_daemon('computer-daemon');a=f.confirmed(a);b=f.confirmed(b)
        recovered=submit(f,computer(f,a,[{'type':'write_file','path':f.worker.root+'/probe-recovered.txt','text':'probe recovered'}]))
        process.task(f,recovered,'completed')
        m02.require(f.worker.call('read',path=f.worker.root+'/probe-recovered.txt')=='probe recovered','incomplete probe prevented guest work')
        details.update(runtime=f.runtime,worker=f.workers())
    with f.check('failed_computer_probe_recovery') as details:
        f.stop(f.daemon)
        configuration=json.loads(f.worker.call('read',path=f.worker.root+'/agent/computer.json'))
        configuration['ceilings']['processes']=32
        f.worker.call('write',files={f.worker.root+'/agent/computer.json':json.dumps(configuration)})
        f.start_daemon('computer-low-resource-probe');a=f.confirmed(a);b=f.confirmed(b)
        rejected=f.owner('POST','/v1/tasks',computer(f,a,[{'type':'write_file','path':f.worker.root+'/failed-probe.txt','text':'must not run'}]),status=409)
        m02.require(f.worker.call('read',path=f.worker.root+'/agent/computer-probe/manifest.json',optional=True) is not None,'failed probe did not retain recovery state')
        f.stop(f.daemon)
        configuration['ceilings']['processes']=128
        f.worker.call('write',files={f.worker.root+'/agent/computer.json':json.dumps(configuration)})
        f.start_daemon('computer-probe-recovered');a=f.confirmed(a);b=f.confirmed(b)
        recovered=submit(f,computer(f,a,[{'type':'write_file','path':f.worker.root+'/stopped-probe-recovered.txt','text':'stopped probe recovered'}]))
        process.task(f,recovered,'completed')
        m02.require(f.worker.call('read',path=f.worker.root+'/stopped-probe-recovered.txt')=='stopped probe recovered','stopped probe prevented guest work')
        details.update(rejected=rejected,recovered=recovered['id'])
    source="""from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
import time,signal,sys
signal.signal(signal.SIGTERM,lambda *_:sys.exit(0))
HTML=PAYLOAD
class Handler(BaseHTTPRequestHandler):
 def do_GET(self):
  if self.path=='/hang':
   time.sleep(90);return
  if self.path=='/effect':
   with open(__file__+'.effects','a') as f:f.write('one\\n')
  self.send_response(200);self.send_header('Content-Type','text/html');self.end_headers();self.wfile.write(HTML)
 def log_message(self,*a):pass
server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
Path(__file__+'.port').write_text(str(server.server_port))
server.serve_forever()
""".replace('PAYLOAD',repr(HTML))
    server=f.worker.root+'/fixture-web.py';f.worker.call('write',files={server:source})
    f.launch(f.worker,'fixture-web',['/usr/bin/python3',server])
    port=f.poll(lambda:f.worker.call('read',path=server+'.port',optional=True),lambda v:v is not None,'fixture origin')
    origin='http://127.0.0.1:'+port
    with f.check('browser_text_screenshot_and_duplicate') as details:
        request=computer(f,a,[{'type':'navigate','url':origin+'/effect'},{'type':'fill','selector':'#login','text':'shared-fixture'},{'type':'click','selector':'#save'},{'type':'capture_text','selector':'#state[data-ready="saved"]','output':'text'},{'type':'screenshot','output':'screen'}],[output('text'),output('screen','png',8388608)])
        original=f.owner('POST','/v1/tasks',request)
        changed=json.loads(json.dumps(request));changed['work']['spec']['viewport']['width']=900
        f.owner('POST','/v1/tasks',changed,status=409)
        process.approve(f,original);completed=process.task(f,original,'completed');completed=ready(f,completed)
        text,_=download(f,completed,'text',b'login=shared-fixture|shared-fixture|shared-fixture')
        image,image_path=download(f,completed,'screen');m02.require(image['prefix'].startswith('89504e470d0a1a0a'),'invalid PNG')
        f.owner('POST','/v1/tasks',request)
        m02.require(f.worker.call('read',path=server+'.effects')=='one\n','duplicate repeated navigation')
        denied=f.manager.call('download',address=f.listen,path=image_path,token='');m02.require(denied['status']==401,'download accepted without authentication')
        details.update(task=completed,text=text,image=image,unauthenticated=denied['status'])
    with f.check('global_and_per_bot_backpressure_and_browser_cancel') as details:
        hung=submit(f,computer(f,a,[{'type':'navigate','url':origin+'/hang'}],runtime_seconds=30))
        process.task(f,hung,'running')
        scopes=f.poll(lambda:f.worker.call('scopes',root=f.worker.root),lambda v:any(s['scope']['execution']==hung['command']['execution'] and s['values']['cgroup.procs'] and len(s['values']['cgroup.procs'].split())>2 for s in v),'Chromium scope')
        scope=next(s for s in scopes if s['scope']['execution']==hung['command']['execution'])
        pids=[int(v) for v in scope['values']['cgroup.procs'].split()]
        m02.require(len(pids)>2,'real Chromium descendants missing')
        queued=submit(f,process.request(f,b,['-c','print("shell preserved")']))
        queued_same=submit(f,computer(f,a,[{'type':'navigate','url':origin+'/'}]))
        for t in (queued,queued_same):
            value=f.owner('GET','/v1/tasks/'+t['id']);m02.require(not value['approval']['consumed'],'occupied approval consumed')
        cancelled=process.cancel(f,hung)
        after=f.worker.call('liveness',pids=pids);m02.require(not any(v['live'] for v in after),'Chromium descendant remains live')
        process.task(f,queued,'completed');process.task(f,queued_same,'completed')
        details.update(scope=scope,after=after,cancelled=cancelled)
    guest=m02.BASE+'/guest-'+f.output.name+'.txt'
    with f.check('guest_files_and_shared_state_across_bots_and_worker_restart') as details:
        write=submit(f,computer(f,b,[{'type':'write_file','path':guest,'text':'approved guest file\n'},{'type':'read_file','path':guest,'output':'file'},{'type':'navigate','url':origin+'/'},{'type':'capture_text','selector':'#state[data-ready="saved"]','output':'state'}],[output('file'),output('state')]))
        completed=ready(f,process.task(f,write,'completed'));download(f,completed,'file',b'approved guest file\n');download(f,completed,'state',b'login=shared-fixture|shared-fixture|shared-fixture')
        f.stop(f.daemon);f.start_daemon('computer-restarted');a=f.confirmed(a);b=f.confirmed(b)
        second=submit(f,computer(f,a,[{'type':'read_file','path':guest,'output':'file'},{'type':'navigate','url':origin+'/'},{'type':'capture_text','selector':'#state[data-ready="saved"]','output':'state'}],[output('file'),output('state')]))
        completed=ready(f,process.task(f,second,'completed'));download(f,completed,'file',b'approved guest file\n');download(f,completed,'state',b'login=shared-fixture|shared-fixture|shared-fixture');details.update(task=completed,guest=guest)
    with f.check('missing_output_is_explicit') as details:
        missing=submit(f,computer(f,a,[{'type':'export_file','path':guest+'.missing','output':'missing'}],[output('missing')]))
        failed=ready(f,process.task(f,missing,'failed'));m02.require(failed['outputs'][0]['state']=={'type':'unavailable','reason':'missing'},'missing output not explicit');details.update(task=failed)
    with f.check('runtime_identity_mutation_refuses_launch') as details:
        request=computer(f,a,[{'type':'navigate','url':origin+'/effect'}])
        request['work']['spec']['runtime']['sha256']='b'*64
        refused=submit(f,request)
        time.sleep(.5)
        value=f.owner('GET','/v1/tasks/'+refused['id']);m02.require(not value['approval']['consumed'],'changed runtime consumed approval')
        process.cancel(f,refused);details.update(task=value)
    with f.check('browser_deadline_and_revocation') as details:
        limited=submit(f,computer(f,a,[{'type':'navigate','url':origin+'/hang'}],runtime_seconds=5))
        result=process.task(f,limited,'failed');m02.require(result['outcome']['reason']=='limit','hang did not reach resource deadline')
        revoked=submit(f,computer(f,a,[{'type':'navigate','url':origin+'/hang'}],runtime_seconds=30))
        process.task(f,revoked,'running');f.owner('DELETE','/v1/tasks/'+revoked['id']+'/approval')
        process.task(f,revoked,'cancelled')
        absolute=submit(f,computer(f,a,[{'type':'navigate','url':origin+'/hang'}],deadline_seconds=6))
        absolute_result=process.task(f,absolute,'failed');m02.require(absolute_result['outcome']['reason']=='limit','absolute deadline lost')
        details.update(deadline=result,absolute_deadline=absolute_result,revoked=f.owner('GET','/v1/tasks/'+revoked['id']))
    with f.check('active_browser_crash_remains_uncertain_without_replay') as details:
        before=f.worker.call('read',path=server+'.effects')
        crashed=submit(f,computer(f,a,[{'type':'navigate','url':origin+'/effect'},{'type':'navigate','url':origin+'/hang'}],runtime_seconds=30))
        f.poll(lambda:f.worker.call('read',path=server+'.effects'),lambda v:v==before+'one\n','crash effect')
        scopes=f.poll(lambda:f.worker.call('scopes',root=f.worker.root),lambda v:any(s['scope']['execution']==crashed['command']['execution'] and s['values']['cgroup.procs'] and len(s['values']['cgroup.procs'].split())>2 for s in v),'crash Chromium')
        scope=next(s for s in scopes if s['scope']['execution']==crashed['command']['execution'])
        pids=[int(v) for v in scope['values']['cgroup.procs'].split()]
        f.worker.call('kill',unit=f.daemon.unit);f.daemon.stopped=True
        f.start_daemon('computer-crash-recovered');a=f.confirmed(a);b=f.confirmed(b)
        uncertain=process.task(f,crashed,'awaiting_attention')
        after=f.worker.call('liveness',pids=pids);m02.require(not any(v['live'] for v in after),'crash recovery left descendants')
        m02.require(f.worker.call('read',path=server+'.effects')==before+'one\n','crash recovery repeated browser effect')
        process.cancel(f,crashed);details.update(task=uncertain,scope=scope,after=after)
    with f.check('resumable_transfer_and_manager_restart') as details:
        large=guest+'.large';data=b'x'*8388608
        shell=submit(f,process.request(f,b,['-c',f"open({large!r},'wb').write(b'x'*8388608)"]))
        process.task(f,shell,'completed')
        lost=submit(f,computer(f,b,[{'type':'export_file','path':large,'output':'lost'}],[output('lost','octet_stream',8388608)]))
        process.task(f,lost,'completed')
        f.poll(lambda:f.manager.call('database',path=f.manager.root+'/state/manager.sqlite3',query='SELECT id,slot,offset,state,reserved FROM artifacts'),lambda rows:any(r[1]=='lost' and r[2]>0 and r[3]=='transferring' for r in rows),'snapshot transfer')
        f.worker.call('remove_snapshot',root=f.worker.root,execution=lost['command']['execution'],slot='lost')
        unavailable=ready(f,lost);m02.require(unavailable['state']=='completed' and unavailable['outputs'][0]['state']=={'type':'unavailable','reason':'corrupt'},'snapshot loss changed execution or remained available')
        damaged=submit(f,computer(f,b,[{'type':'export_file','path':large,'output':'bad'},{'type':'read_file','path':guest,'output':'next'}],[output('bad','octet_stream',8388608),output('next')]))
        process.task(f,damaged,'completed')
        damaged_rows=f.poll(lambda:f.manager.call('database',path=f.manager.root+'/state/manager.sqlite3',query='SELECT id,slot,offset,state,reserved FROM artifacts'),lambda rows:any(r[1]=='bad' and r[2]>0 and r[3]=='transferring' for r in rows),'corrupt receiving transfer')
        damaged_id=next(r[0] for r in damaged_rows if r[1]=='bad')
        f.manager.call('corrupt_partial',root=f.manager.root,id=damaged_id)
        recovered=ready(f,damaged)
        m02.require(recovered['state']=='completed' and next(o for o in recovered['outputs'] if o['declaration']['slot']=='bad')['state']=={'type':'unavailable','reason':'corrupt'},'corrupt transfer did not finish unavailable')
        download(f,recovered,'next',b'approved guest file\n')
        export=submit(f,computer(f,b,[{'type':'export_file','path':large,'output':'large'}],[output('large','octet_stream',8388608)]))
        completed=process.task(f,export,'completed')
        partial=f.poll(lambda:f.manager.call('database',path=f.manager.root+'/state/manager.sqlite3',query='SELECT id,slot,offset,state,reserved FROM artifacts'),lambda rows:any(r[1]=='large' and r[2]>0 and r[3]=='transferring' for r in rows),'partial transfer')
        process.restart_manager(f,"manager-restarted-"+str(uuid.uuid4())[:8])
        completed=ready(f,completed);result,path=download(f,completed,'large',data)
        process.restart_manager(f,"manager-restarted-"+str(uuid.uuid4())[:8]);result2,_=download(f,completed,'large',data)
        wrong='/v1/tasks/'+shell['id']+'/artifacts/'+next(o['state']['id'] for o in completed['outputs'])
        denied=f.manager.call('download',address=f.listen,path=wrong,token=f.owner_token);m02.require(denied['status']==404,'unrelated task download accepted')
        f.manager.call('corrupt',root=f.manager.root,id=completed['outputs'][0]['state']['id'])
        corrupt=f.manager.call('download',address=f.listen,path=path,token=f.owner_token);m02.require(corrupt['status']!=200,'corrupt artifact served')
        process.restart_manager(f,"manager-restarted-"+str(uuid.uuid4())[:8])
        corrupt_task=f.owner('GET','/v1/tasks/'+completed['id']);m02.require(corrupt_task['outputs'][0]['state']=={'type':'unavailable','reason':'corrupt'},'corruption not durable')
        details.update(partial=partial,download=result,after_restart=result2,corrupt=corrupt_task,snapshot_loss=unavailable,receiving_corruption=recovered)
    with f.check('unavailable_capability_rejected'):
        f.stop(f.daemon)
        config=f.worker.root+'/agent/computer.json';f.worker.call('move',source=config,destination=config+'.disabled')
        f.start_daemon('computer-disabled');a=f.confirmed(a)
        f.owner('POST','/v1/tasks',computer(f,a,[{'type':'navigate','url':origin+'/'}]),status=409)
        f.stop(f.daemon);f.worker.call('move',source=config+'.disabled',destination=config)
    with f.check('profile_corruption_refuses_without_erasing_state'):
        f.worker.call('corrupt_profile',root=f.worker.root)
        f.start_daemon('computer-corrupt-profile');a=f.confirmed(a)
        f.owner('POST','/v1/tasks',computer(f,a,[{'type':'navigate','url':origin+'/'}]),status=409)
        m02.require(f.worker.call('read',path=f.worker.root+'/agent/automation-profile/Default/Cookies')=='corrupt cookie database','profile corruption was silently erased')
        m02.require(f.worker.call('read',path=guest)=='approved guest file\n','guest state changed')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base',required=True);p.add_argument('--runtime',required=True);p.add_argument('--runtime-manifest',type=Path,required=True);p.add_argument('--cert-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--manager-ssh',default='botstral-manager');p.add_argument('--worker-ssh',default='botstral-computer');p.add_argument('--manager-address',default='192.168.1.40:43124')
    args=p.parse_args();os.umask(0o077);m02.BASE=args.base;m02.TIMEOUT=30
    run=str(uuid.uuid4());out=args.output_dir/run;out.mkdir(mode=0o700,parents=True)
    certificates={n:(args.cert_dir/n).read_text() for n in ('ca.pem','manager.pem','manager.key','worker.pem','worker.key')}
    root=args.base+'/runs/'+run
    f=m02.Fixture('production',Remote(args.manager_ssh,root),Remote(args.worker_ssh,root),args.manager_address,out,[m02.PASSWORD,m02.VAULT,*certificates.values()])
    try:m02.setup(f,certificates);exercise(f,certificates,args)
    except Exception as e:m02.record('fixture','failed',str(e) if isinstance(e,m02.CheckFailure) else repr(e))
    finally:
        f.finish()
        try:m02.record('process_cleanup','passed',f.worker.call('cleanup_processes',root=root,units=[p.unit for p in f.processes if p.remote is f.worker]))
        except Exception as e:m02.record('process_cleanup','failed',repr(e))
    required={'production.'+v for v in ('computer_enrollment_and_runtime_probe','failed_computer_probe_recovery','browser_text_screenshot_and_duplicate','global_and_per_bot_backpressure_and_browser_cancel','guest_files_and_shared_state_across_bots_and_worker_restart','missing_output_is_explicit','runtime_identity_mutation_refuses_launch','browser_deadline_and_revocation','active_browser_crash_remains_uncertain_without_replay','resumable_transfer_and_manager_restart','unavailable_capability_rejected','profile_corruption_refuses_without_erasing_state')}
    missing=required-{v['name'] for v in m02.RESULTS if v['status']=='passed'}
    if missing:m02.record('required_contract','failed',{'missing':sorted(missing)})
    passed=not missing and all(v['status']=='passed' for v in m02.RESULTS)
    m02.private_json(out/'results.json',{'passed':passed,'checks':m02.RESULTS,'root':root})
    print(json.dumps({'passed':passed,'evidence':str(out.resolve())}));return 0 if passed else 1

if __name__=='__main__':sys.exit(main())
