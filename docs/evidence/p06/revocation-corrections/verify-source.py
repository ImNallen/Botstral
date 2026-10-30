import hashlib,json,pathlib,subprocess
p=pathlib.Path(__file__).resolve().parent
base='bf470fb00e98ed6e519b02f7a314fe48a808bdc1'
head='086b1acb41226be3e5ace6055be6f765c070b8fa'
def git(*args):return subprocess.check_output(['git',*args],text=True)
assert git('rev-parse','HEAD').strip()==head
subprocess.run(['git','merge-base','--is-ancestor',base,head],check=True)
assert git('diff','--binary')==''
assert git('status','--porcelain')==''
file='manager/src/manager/state/connectors.rs'
a=git('show',base+':'+file)
b=pathlib.Path(file).read_text()
def method(s,name):
 pos=s.index('fn '+name)
 start=s.rfind('\n',0,pos)+1
 i=s.index('{',pos)+1
 depth=1
 while depth:
  depth+=(s[i]=='{')-(s[i]=='}')
  i+=1
 return s[start:i]
checks={name:{'identical_to_accepted_base':method(a,name)==method(b,name),'sha256':hashlib.sha256(method(b,name).encode()).hexdigest()} for name in ['known_connector_secrets','change_connection','connection_secret']}
assert all(x['identical_to_accepted_base'] for x in checks.values())
result=dict(head=head,accepted_base=base,accepted_base_is_ancestor=True,status_clean=True,tracked_diff_empty=True,inherited_methods=checks,base_evidence='Source equality only. The accepted base was not executed.')
(p/'final-state.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result))
