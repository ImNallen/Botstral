import pathlib,subprocess,sys,json
root=pathlib.Path.cwd(); p=root/'.audit/p06-recheck'
target=root/'manager/src/manager/state/runs/tests/publication_tests.rs'
original=target.read_bytes()
try:
 target.write_bytes(original+('\ninclude!('+json.dumps(str(p/'publication-probes.rs'))+');\n').encode())
 r=subprocess.run([sys.executable,str(p/'run.py'),'adversarial','cargo','test','-p','botstral-manager','--locked','--lib','recheck_','--','--nocapture'])
finally:
 target.write_bytes(original)
sys.exit(r.returncode)
