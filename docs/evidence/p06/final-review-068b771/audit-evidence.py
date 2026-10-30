import pathlib,json,hashlib,subprocess
root=pathlib.Path('/Users/nallen/.t3/worktrees/Botstral/t3-010ce7cb')
local=pathlib.Path.cwd(); audit=local/'.audit/p06-public-context-review'
results=[]
for owner,folder in [(local,'packaged-native-linux-controlled'),(local,'linux-final-password-totp'),(root,'linux-public-context-final')]:
 e=owner/'docs/evidence/p06'/folder
 commands=json.loads((e/'commands.json').read_text())
 assert all(r['exit']==0 for r in commands)
 assert any(r['log']=='integration.log' for r in commands)
 assert any(r['log']=='remove-owned-container.log' for r in commands)
 assert (e/'binfmt-before.log').read_bytes()==(e/'binfmt-after.log').read_bytes()
 rows=[json.loads(line) for line in (e/'integration.log').read_text().splitlines()]
 sign=next(r for r in rows if r['check']=='sign_in_result')
 assert sign['task']['state']=='completed'
 assert sign['submissions']==[dict(valid=True,path='/session',method='POST')]
 assert any(r['state']=='applied' for r in sign['reviews'])
 for name in ['missing_client_certificate_refused','changed_generation_refused','same_uid_outside_scope_refused','repeated_tls_release_refused']:
  assert any(r['check']==name for r in rows),name
 totp=[r for r in rows if r['check']=='production_linux_totp_passed']
 validation=[r for r in rows if r['check']=='factor_validation']
 if folder!='packaged-native-linux-controlled':
  assert len(totp)==len(validation)==2
  assert all(r['code_valid'] and r['session_valid'] for r in validation)
  assert all(r['fresh_approval'] and r['review']['state']=='applied' and r['task']['state']=='completed' for r in totp)
  prep=[r for r in rows if r['check']=='totp_review_prepared']
  assert len(prep)==2 and all(r['review']['state']=='awaiting_approval' for r in prep)
 source=json.loads((e/'source.json').read_text())
 results.append(dict(folder=str(e),source=source,password_posts=1,fresh_totp=len(totp),transport_refusals=4,all_command_exits_zero=True,binfmt_equal=True,integration_sha256=hashlib.sha256((e/'integration.log').read_bytes()).hexdigest()))
(audit/'evidence-audit.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps(results,indent=2))
