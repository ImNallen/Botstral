import hashlib,json,pathlib,subprocess
p=pathlib.Path(__file__).resolve().parent
head='b45c7ed575d3009de8a7b3fcf8f64da76e5caa32'
base='bf470fb00e98ed6e519b02f7a314fe48a808bdc1'
old='086b1acb41226be3e5ace6055be6f765c070b8fa'
packaged='5d5da1814ad120eb5895a12fed70afa5fdfa092d'
def git(*args): return subprocess.check_output(['git',*args],text=True)
assert git('rev-parse','HEAD').strip()==head
subprocess.run(['git','merge-base','--is-ancestor',base,head],check=True)
assert git('diff')=='' and git('status','--porcelain')==''
source='manager/src/manager/state/connectors.rs'
def method(rev,name):
 s=git('show',f'{rev}:{source}'); a=s.index(f'fn {name}('); start=s.index('{',a);depth=0
 for i in range(start,len(s)):
  if s[i]=='{':depth+=1
  elif s[i]=='}':
   depth-=1
   if depth==0:return s[a:i+1]
assert method(base,'connection_secret')==method(head,'connection_secret')
assert method(base,'change_connection')==method(head,'change_connection')
assert method(base,'known_connector_secrets')==method(old,'known_connector_secrets')
assert git('diff','--name-only',old,head,'--','agent','link','desktop','mobile')==''
assert git('diff','--name-only',packaged,head,'--','agent/src','agent/runtime','link','desktop/src-tauri')==''
result=dict(head=head,accepted_base=base,base_is_ancestor=True,clean_status=True,tracked_diff_sha256=hashlib.sha256(git('diff').encode()).hexdigest(),scanner_old_equals_base=True,connection_secret_and_change_connection_unchanged_from_base=True,native_protocol_agent_unchanged_since_packaged=True,component_changes_since_catalog=git('diff','--name-only',old,head,'--','manager','agent','link','desktop','mobile').splitlines())
(p/'final-state.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
