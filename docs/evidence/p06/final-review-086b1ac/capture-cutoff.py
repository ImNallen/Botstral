import datetime,hashlib,json,pathlib,subprocess
p=pathlib.Path(__file__).resolve().parent
root=pathlib.Path('/Users/nallen/.t3/worktrees/Botstral/t3-010ce7cb')
e=root/'docs/evidence/p06/20261030T002914969649Z'
commands=json.loads((e/'commands.json').read_text())
environment=json.loads((e/'environment.json').read_text())
assert environment['head']=='086b1acb41226be3e5ace6055be6f765c070b8fa'
assert environment['diff_sha256']==hashlib.sha256(b'').hexdigest()
logs={}
for row in commands:
 assert row['exit_code']==0
 name=row['name']+'.log'
 if (e/name).exists():logs[name]=hashlib.sha256((e/name).read_bytes()).hexdigest()
result=dict(cutoff=datetime.datetime.now(datetime.timezone.utc).isoformat(),directory=str(e),environment=environment,completed_commands=commands,log_sha256=logs,broad_complete=len(commands)==14)
(p/'root-driver-cutoff.json').write_text(json.dumps(result,indent=2)+'\n')
canonical=root/'.audit/p06/decisions.tsv'
trail=canonical.read_bytes()
(p/'coordinator-trail-reviewed.tsv').write_bytes(trail)
committed=pathlib.Path('docs/evidence/p06/coordinator-decisions.tsv').read_bytes()
assert trail.startswith(committed)
print(json.dumps({'cutoff':result['cutoff'],'completed_phases':[x['name'] for x in commands],'broad_complete':result['broad_complete'],'canonical_trail_lines':len(trail.splitlines()),'canonical_trail_sha256':hashlib.sha256(trail).hexdigest()}))
