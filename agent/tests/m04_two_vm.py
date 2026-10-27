#!/usr/bin/env python3
"""Run production M04 acceptance with an explicit EU provider cap and isolated fixtures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import time
import uuid
import m02_two_vm as m02
import w01_two_vm as process
import w01_computer_two_vm as computer

MODEL = 'mistral-small-2603'
CONTEXT = 262144
OUTPUT = 1024
INPUT_PRICE = 200000
OUTPUT_PRICE = 800000
CALL_HOLD = (CONTEXT * INPUT_PRICE + 999999) // 1000000 + (OUTPUT * OUTPUT_PRICE + 999999) // 1000000
CALL_PLAN = {'reasoning': 2, 'edit': 3, 'resume': 1, 'vision': 1, 'cancel': 1}


class Remote(computer.Remote):
    def install(self):
        names = ('m02_remote.py', 'w01_remote.py', 'w01_computer_remote.py', 'm04_remote.py')
        sources = {name: Path(__file__).with_name(name).read_text() for name in names}
        sources['seal-vibe.py'] = (Path(__file__).parents[1] / 'vibe/seal.py').read_text()
        bootstrap = "import os,json,pathlib,sys; os.umask(0o077); p=json.load(sys.stdin); r=pathlib.Path(p['root']); r.mkdir(mode=0o700,parents=True,exist_ok=False); [(r/n).write_text(s) for n,s in p['sources'].items()]; print(json.dumps({'ok':True,'value':True}))"
        self.ssh(['python3', '-c', bootstrap], {'root':self.root, 'sources':sources})
    def call(self, op, **values):
        return self.ssh(['python3', '-B', self.root + '/m04_remote.py'], {'op':op, **values})


def limits(calls, deadline=None):
    return {'deadline': deadline or int(time.time()) + 240, 'max_turns':calls,
            'max_total_tokens':calls * (CONTEXT + OUTPUT), 'max_cost_microusd':calls * CALL_HOLD,
            'max_output_tokens':OUTPUT, 'max_operations':1, 'max_concurrent_operations':1}


def configure(f, bot):
    now = int(time.time())
    config = f.owner('PUT', '/v1/inference-configurations', {'bot':bot, 'expected_revision':0, 'policy':{
        'endpoint':'eu', 'model_id':MODEL, 'context_tokens':CONTEXT,
        'acknowledgment':{'model_developer':'mistralai', 'developer_terms_url':'https://legal.mistral.ai/terms/api',
            'privacy_terms_url':'https://legal.mistral.ai/terms/privacy-policy', 'acknowledged_at':now, 'release':{'type':'stable'}},
        'prices':{'input_microusd_per_million_tokens':INPUT_PRICE, 'output_microusd_per_million_tokens':OUTPUT_PRICE,
            'effective_at':now, 'expires_at':now + 3600, 'billing':'context_tokens_including_images_at_acknowledged_ceilings'}}})
    verified = f.owner('POST', '/v1/inference-configurations/verify', {'bot':bot, 'revision':config['revision']})
    m02.require(verified['verification']['type'] == 'verified' and verified['verification']['vision'], 'exact model vision capability not verified')
    return verified


def run(f, session, prompt, calls, policy, images=()):
    request = {'id':str(uuid.uuid4()), 'bot':session['bot_id'], 'session':session['id'], 'configuration_revision':1,
               'prompt':prompt, 'execution_policy':policy, 'limits':limits(calls) | {'max_operations':12}, 'images':list(images)}
    f.owner('POST', '/v1/runs', request)
    def progress():
        value = f.owner('GET', '/v1/runs/' + request['id'])
        if value['summary']['phase']['type'] == 'waiting_for_tool':
            task = f.owner('GET', '/v1/tasks/' + value['summary']['phase']['task'])
            if task['state'] == 'awaiting_approval':
                f.owner('PUT', '/v1/tasks/' + task['id'] + '/approval', {'expires_at':min(int(time.time()) + 90, request['limits']['deadline'])})
        return value
    return f.poll(progress, lambda v: v['summary']['phase']['type'] in ('completed','stopped','awaiting_attention'), 'reasoning result')


def coding_request(f, session, workspace, prompt, lane, resume=None):
    bound = limits(CALL_PLAN[lane])
    return {'id':str(uuid.uuid4()), 'session_id':session['id'], 'conversation_id':None,
        'settings':{'model':MODEL, 'max_turns':bound['max_turns'], 'max_tokens':bound['max_total_tokens'],
                    'deadline':bound['deadline'], 'max_cost_micros':bound['max_cost_microusd']},
        'work':{'type':'vibe', 'spec':{'prompt':prompt, 'limits':bound, 'session':resume or {'type':'new'},
            'policy':{'workspace':workspace, 'runtime':f.vibe_runtime, 'account':{'uid':1000,'gid':1000},
                'resources':{'runtime_seconds':120,'output_bytes':1024,'processes':32,'memory_bytes':268435456},
                'configuration_revision':1, 'model_id':MODEL, 'access':'edit_files_as_guest_account',
                'outputs':[computer.output('answer',maximum=4096) | {'name':'answer.py'},
                           computer.output('transcript',maximum=1048576)]}}}}


def finished_coding(f, request):
    original = computer.submit(f, request)
    result = f.poll(lambda:f.owner('GET','/v1/tasks/'+original['id']), lambda t:t['state'] in ('completed','failed','awaiting_attention'), 'coding exit')
    m02.require(result['state'] == 'completed', 'coding task did not complete')
    output = result['process']['result']['stdout']
    m02.require(output['complete'] and not output['truncated'], 'coding result truncated')
    native = json.loads(bytes.fromhex(output['hex']))
    m02.require(native['exit_code'] == 0 and str(uuid.UUID(native['native_session'])) == native['native_session'], 'invalid native result')
    result = computer.ready(f, result)
    calls = f.owner('GET', '/v1/tasks/' + result['id'] + '/coding-calls')
    m02.require(calls and all(c['phase'] == 'settled' for c in calls), 'coding calls did not settle')
    return result, native, calls


def coding_lifecycle(f, session, args):
    with f.check('coding_cancel_and_pre_call_budget_refusal') as details:
        workspace={'id':str(uuid.uuid4()),'path':f.worker.root+'/cancel-workspace'}
        f.worker.call('mkdir',path=workspace['path'])
        request=coding_request(f,session,workspace,'Create answer.py with a carefully documented implementation of a parser. Work until complete.','cancel')
        pending=computer.submit(f,request)
        def native_scope():
            for candidate in f.worker.call('scopes',root=f.worker.root):
                if candidate['scope']['execution'] != pending['command']['execution']:
                    continue
                pids = [int(v) for v in (candidate['values']['cgroup.procs'] or '').split()]
                observed = f.worker.call('vibe_processes', pids=pids, binary=args.vibe_runtime + '/bin/vibe')
                if any(p['vibe_cli'] for p in observed):
                    return {'scope': candidate, 'processes': observed}
            return None
        observed=f.poll(native_scope,lambda value:value is not None,'actual Vibe CLI in coding scope')
        scope=observed['scope']
        cancelled=process.cancel(f,pending)
        pids=[int(v) for v in scope['values']['cgroup.procs'].split()]
        m02.require(not any(v['live'] for v in f.worker.call('liveness',pids=pids)),'coding descendants survived cancellation')
        stopped = next(s for s in f.worker.call('scopes', root=f.worker.root) if s['scope']['execution'] == pending['command']['execution'])
        events = stopped['values']['cgroup.events']
        m02.require(events is None or 'populated 0' in events.splitlines(), 'cancelled scope remains populated')
        refused=coding_request(f,session,{'id':str(uuid.uuid4()),'path':f.worker.root+'/refused'},'Do not run.','edit')
        refused['work']['spec']['limits']['max_cost_microusd']=1
        rejection=f.owner('POST','/v1/tasks',refused,status=409)
        m02.require(rejection['error'] == 'inference_budget', 'unexpected coding budget refusal')
        f.owner('GET', '/v1/tasks/' + refused['id'], status=404)
        f.owner('GET', '/v1/tasks/' + refused['id'] + '/coding-calls', status=404)
        details.update(stopped_scope=stopped,refused_task=refused['id'],cancelled=cancelled,native_processes=observed['processes'],provider_calls=f.owner('GET','/v1/tasks/'+pending['id']+'/coding-calls'),budget_refused=True)
    return cancelled


def lifecycle_restart(f, task, session):
    with f.check('manager_restart_keeps_cancelled_result_and_never_reexecutes') as details:
        path = '/v1/tasks/' + task['id']
        before = f.owner('GET', path + '/coding-calls')
        executions = {s['scope']['execution'] for s in f.worker.call('scopes', root=f.worker.root)}
        process.restart_manager(f, 'm04-lifecycle-manager-restarted')
        reattached = f.phase_wait('ready', f.generation + 1)
        f.generation = reattached['generation']
        f.confirmed(session)
        after = f.owner('GET', path + '/coding-calls')
        restored = f.owner('GET', path)
        m02.require(before == after, 'restart changed cancelled coding attempts')
        m02.require(restored['state'] == 'cancelled' and restored['process'] == task['process'], 'restart changed cancelled process result')
        time.sleep(2)
        m02.require(f.owner('GET', path + '/coding-calls') == before, 'delayed coding replay after restart')
        m02.require(f.owner('GET', path)['process'] == task['process'], 'delayed process replay after restart')
        scopes = f.worker.call('scopes', root=f.worker.root)
        m02.require({s['scope']['execution'] for s in scopes} == executions, 'restart created a native execution')
        m02.require(all(s['values']['cgroup.events'] is None or 'populated 0' in s['values']['cgroup.events'].splitlines() for s in scopes), 'restart repopulated a stopped scope')
        details.update(calls=after, task=restored, scopes_after_restart=scopes, worker_after_restart=reattached)


def exercise(f, certificates, args):
    plan = {'cancel': 1} if args.suite == 'coding-lifecycle' else CALL_PLAN
    with f.check('runtime_and_worker') as details:
        f.enroll(f.invitation(certificates), certificates)
        a = f.session('M04 A')
        b = f.session('M04 B')
        f.runtime = hashlib.sha256(args.runtime_manifest.read_bytes()).hexdigest()
        f.vibe_runtime = f.worker.call('seal_vibe', script=f.worker.root+'/seal-vibe.py', runtime=args.vibe_runtime, manifest=f.worker.root+'/vibe-runtime.json')['sha256']
        f.worker.call('write',files={
            f.worker.root+'/agent/process.json':json.dumps({'profile':'linux_systemd_v1','account':{'uid':1000,'gid':1000},'ceilings':process.limits() | {'runtime_seconds':120}}),
            f.worker.root+'/agent/computer.json':json.dumps({'account':{'uid':1000,'gid':1000},'runtime':{'sha256':f.runtime},'directory':args.runtime,'ceilings':{'runtime_seconds':60,'output_bytes':1024,'processes':128,'memory_bytes':1073741824}}),
            f.worker.root+'/agent/vibe.json':json.dumps({'account':{'uid':1000,'gid':1000},'directory':args.vibe_runtime,'manifest':f.worker.root+'/vibe-runtime.json','runtime':f.vibe_runtime,'ceilings':process.limits() | {'runtime_seconds':120}})})
        f.start_daemon('m04-worker'); a=f.confirmed(a); b=f.confirmed(b)
        details.update(vibe_runtime=f.vibe_runtime,computer_runtime=f.runtime)
    if not args.live:
        return {'prepared':True,'live_acceptance':False}
    with f.check('provider_configuration') as details:
        budget=f.manager.call('acceptance_budget',path=args.budget_ledger,run=f.output.name,cap_microusd=args.cap_microusd,previous_reserved_microusd=args.previous_reserved_microusd,reservation_microusd=CALL_HOLD*sum(plan.values()))
        f.owner('POST','/v1/vault/unlock',{'passphrase':m02.VAULT},status=204)
        f.manager.call('provision_provider',address=f.listen,token=f.owner_token,provider_key_file=args.provider_key_file)
        details.update(budget=budget,configuration=configure(f,a['bot_id']),cap_microusd=args.cap_microusd,worst_case_microusd=CALL_HOLD*sum(plan.values()))
    if args.suite == 'coding-lifecycle':
        lifecycle_restart(f, coding_lifecycle(f, a, args), a)
        return {'live_acceptance': False}
    with f.check('browser_shared_file_and_owner_artifacts') as details:
        challenge = str(secrets.randbelow(900000) + 100000)
        page = f.worker.root+'/challenge.html'
        shared = f.worker.root+'/shared.txt'
        html = '<html><body style="font: bold 90px monospace;background:white;color:black"><p id="proof" style="font:16px monospace">M04 reasoning browser proof</p><input id="value"><button id="save" onclick="document.getElementById(\'result\').textContent=document.getElementById(\'value\').value">Save</button><div id="result"></div><canvas id="challenge" width="600" height="120"></canvas><script>let c=document.getElementById("challenge").getContext("2d");c.font="bold 90px monospace";c.fillText("'+challenge+'",5,95)</script></body></html>'
        f.worker.call('write',files={page:html})
        web = f.worker.root + '/web.py'
        f.worker.call('write',files={web:'import http.server,os,signal,sys\nsignal.signal(signal.SIGTERM,lambda *_:sys.exit(0))\nos.chdir('+repr(f.worker.root)+')\nhttp.server.ThreadingHTTPServer(("127.0.0.1",'+str(args.page_port)+'),http.server.SimpleHTTPRequestHandler).serve_forever()\n'})
        f.launch(f.worker,'page',['/usr/bin/python3',web])
        origin='http://127.0.0.1:'+str(args.page_port)+'/challenge.html'
        request=computer.computer(f,a,[{'type':'navigate','url':origin},{'type':'fill','selector':'#value','text':'M04 shared browser'},{'type':'click','selector':'#save'},{'type':'capture_text','selector':'#result','output':'text'},{'type':'screenshot','output':'screen'},{'type':'write_file','path':shared,'text':'M04 shared file\n'}],[computer.output('text'),computer.output('screen','png',1048576)])
        first=computer.ready(f,process.task(f,computer.submit(f,request),'completed'))
        computer.download(f,first,'text',b'M04 shared browser')
        image_download,_=computer.download(f,first,'screen')
        second=computer.submit(f,computer.computer(f,b,[{'type':'read_file','path':shared,'output':'file'}],[computer.output('file')]))
        second=computer.ready(f,process.task(f,second,'completed'));computer.download(f,second,'file',b'M04 shared file\n')
        policy={'computer':{'profile':'linux_playwright_v1','account':{'uid':1000,'gid':1000},'runtime':{'sha256':f.runtime},'limits':{'runtime_seconds':60,'output_bytes':1024,'processes':128,'memory_bytes':1073741824},'viewport':{'width':800,'height':600},'outputs':[computer.output('text')]},'process':None}
        arguments={'operations':[{'type':'navigate','url':origin},{'type':'capture_text','selector':'#proof','output':'text'}],'outputs':['text']}
        reasoning=run(f,a,'Call computer exactly once with these arguments, preserving operation order and output slot names: '+json.dumps(arguments)+'. After its result, answer briefly without another tool call.',CALL_PLAN['reasoning'],policy)
        m02.require(reasoning['summary']['phase']['type']=='completed' and any(t['intent'] for t in reasoning['turns']),'live reasoning did not execute a browser child')
        results = [json.loads(t['intent']['result']) for t in reasoning['turns'] if t['intent'] and t['intent']['result']]
        m02.require(any(a.get('untrusted_text') == 'M04 reasoning browser proof' for r in results for a in r['untrusted_tool_result']['artifacts']), 'reasoning did not receive the observed browser text')
        details.update(first=first['id'],second=second['id'],shared=shared,image=image_download,reasoning=reasoning)
    with f.check('actual_vision') as details:
        image=next(o for o in first['outputs'] if o['declaration']['slot']=='screen')['state']
        references=[{'task':first['id'],'artifact':image['id'],'slot':'screen','sha256':image['content']['sha256']}]
        vision=run(f,a,'Read the six-digit number drawn in the canvas. Reply with only its digits. Do not use tools.',CALL_PLAN['vision'],
            {'computer':None,'process':None,
             'vision':{'allowed_slots':['screen'],'max_images':1,'max_image_bytes':1048576,'max_total_pixels':1638400}},references)
        answer=vision['turns'][-1]['response']['content'].strip()
        m02.require(answer==challenge,'vision challenge answer differed')
        details.update(run=vision,matched=True)
    with f.check('vibe_edit_separate_test_and_exact_resume') as details:
        workspace={'id':str(uuid.uuid4()),'path':f.worker.root+'/coding-workspace'}
        f.worker.call('mkdir',path=workspace['path'])
        f.worker.call('write',files={workspace['path']+'/answer.py':'def answer():\n    return 0\n'})
        edited,native,calls=finished_coding(f,coding_request(f,a,workspace,'Use edit to change return 0 to return 42 in answer.py. Do not run tests. Then finish.','edit'))
        test=process.request(f,a,['-c','import runpy; assert runpy.run_path('+repr(workspace['path']+'/answer.py')+')["answer"]() == 42'])
        tested=process.task(f,computer.submit(f,test),'completed')
        resumed,native2,calls2=finished_coding(f,coding_request(f,a,workspace,'Reply with exactly resumed. Do not call any tool or modify files.','resume',{'type':'resume','task':edited['id'],'native_session':native['native_session']}))
        m02.require(native2['native_session']==native['native_session'],'resume changed native identity')
        f.stop(f.daemon)
        computer.download(f,edited,'answer',b'def answer():\n    return 42\n')
        f.start_daemon('m04-worker-returned');a=f.confirmed(a);b=f.confirmed(b)
        details.update(edit=edited['id'],test=tested['id'],resume=resumed['id'],native_session=native['native_session'],calls=calls+calls2)
    coding_lifecycle(f, a, args)
    with f.check('manager_restart_keeps_results_and_never_reexecutes') as details:
        before=f.owner('GET','/v1/tasks/'+edited['id']+'/coding-calls')
        process.restart_manager(f,'m04-manager-restarted')
        after=f.owner('GET','/v1/tasks/'+edited['id']+'/coding-calls')
        m02.require(before==after,'restart changed settled coding attempts')
        computer.download(f,edited,'answer',b'def answer():\n    return 42\n')
        details.update(calls=after,task=edited['id'])
    return {'live_acceptance':True}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base',required=True,help='Isolated remote base containing bin/production binaries on both VMs.')
    parser.add_argument('--runtime',default='/home/botstral/w01-runtime-76b03bf1')
    parser.add_argument('--runtime-manifest',required=True,type=Path)
    parser.add_argument('--vibe-runtime',default='/home/botstral/m04-vibe-2.24.2')
    parser.add_argument('--cert-dir',required=True,type=Path)
    parser.add_argument('--output-dir',required=True,type=Path)
    parser.add_argument('--manager-ssh',default='botstral-manager')
    parser.add_argument('--worker-ssh',default='botstral-computer')
    parser.add_argument('--manager-address',default='192.168.1.40:43124')
    parser.add_argument('--page-port',type=int,default=43871)
    parser.add_argument('--live',action='store_true')
    parser.add_argument('--suite', choices=['full', 'coding-lifecycle'], default='full', help='The lifecycle suite checks cancellation, budget refusal and restart only.')
    parser.add_argument('--endpoint',choices=['eu'],default='eu')
    parser.add_argument('--provider-key-file',help='Private key file already provisioned on Manager. Never copied to Computer.')
    parser.add_argument('--budget-ledger',help='Persistent Manager-side ledger. A run reserves its full plan before paid work, across retries.')
    parser.add_argument('--previous-reserved-microusd',type=int,help='Prior paid attempts and unknown holds in this acceptance campaign; recorded permanently in the ledger.')
    parser.add_argument('--cap-microusd',type=int,default=1000000)
    args=parser.parse_args()
    plan = {'cancel': 1} if args.suite == 'coding-lifecycle' else CALL_PLAN
    if args.live and (not args.provider_key_file or not args.budget_ledger or args.previous_reserved_microusd is None):parser.error('--live requires --provider-key-file, --budget-ledger and --previous-reserved-microusd')
    if not 0<args.cap_microusd<=1000000 or not 0 <= (args.previous_reserved_microusd or 0) or CALL_HOLD*sum(plan.values())+(args.previous_reserved_microusd or 0)>args.cap_microusd:parser.error('worst-case acceptance reservations exceed the authorized cap')
    if args.manager_ssh==args.worker_ssh:parser.error('separate VMs are required')
    os.umask(0o077);m02.BASE=args.base;m02.TIMEOUT=180
    run_id=str(uuid.uuid4());out=args.output_dir/run_id;out.mkdir(mode=0o700,parents=True)
    certificates={n:(args.cert_dir/n).read_text() for n in ('ca.pem','manager.pem','manager.key','worker.pem','worker.key')}
    root=args.base+'/runs/'+run_id
    f=m02.Fixture('production',Remote(args.manager_ssh,root),Remote(args.worker_ssh,root),args.manager_address,out,[m02.PASSWORD,m02.VAULT,*certificates.values()])
    outcome={'live_acceptance':False}
    try:
        m02.setup(f,certificates)
        outcome=exercise(f,certificates,args)
    except Exception as error:
        m02.record('fixture','failed',str(error) if isinstance(error,m02.CheckFailure) else type(error).__name__)
    finally:
        f.finish()
        try:m02.record('process_cleanup','passed',f.worker.call('cleanup_processes',root=root,units=[p.unit for p in f.processes if p.remote is f.worker]))
        except Exception as error:m02.record('process_cleanup','failed',type(error).__name__)
    required={'runtime_and_worker'}
    if args.live:
        required.update(('provider_configuration', 'coding_cancel_and_pre_call_budget_refusal'))
        if args.suite == 'full':
            required.update(('browser_shared_file_and_owner_artifacts', 'actual_vision', 'vibe_edit_separate_test_and_exact_resume', 'manager_restart_keeps_results_and_never_reexecutes'))
        else:
            required.add('manager_restart_keeps_cancelled_result_and_never_reexecutes')
    missing={'production.'+n for n in required}-{v['name'] for v in m02.RESULTS if v['status']=='passed'}
    passed=not missing and all(v['status']=='passed' for v in m02.RESULTS)
    source_root=Path(__file__).resolve().parents[2]
    names=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard','--','Cargo.toml','Cargo.lock','rust-toolchain.toml','agent','manager','link'],cwd=source_root).decode().split('\0')
    sources={name:hashlib.sha256((source_root/name).read_bytes()).hexdigest() for name in sorted(set(names)) if name and '__pycache__' not in Path(name).parts}
    result={'suite':args.suite,'live_checks_passed':args.live and passed,'passed':passed,'live_acceptance':passed and outcome['live_acceptance'],'checks':m02.RESULTS,'missing':sorted(missing),'root':root,'source_hashes':sources,'call_plan':plan,'worst_case_microusd':CALL_HOLD*sum(plan.values()),'cap_microusd':args.cap_microusd}
    m02.private_json(out/'results.json',result)
    print(json.dumps({'passed':passed,'live_acceptance':result['live_acceptance'],'evidence':str(out.resolve())}))
    return 0 if passed else 1


if __name__=='__main__':sys.exit(main())
