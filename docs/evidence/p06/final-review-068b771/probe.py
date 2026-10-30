import pathlib,subprocess,sys,json
root=pathlib.Path.cwd(); p=pathlib.Path(__file__).resolve().parent
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()=='068b7717d93d1b4e7f9a45ad1df6fe4cc9c24897'
pairs=[('manager/src/manager/state/runs/tests/publication_tests.rs','publication-probes.rs'),('manager/src/manager/state/credentials/tests/live/authority.rs','generated-probes.rs')]
originals={root/target:(root/target).read_bytes() for target,_ in pairs}
try:
 for target,fragment in pairs:
  path=root/target; path.write_bytes(originals[path]+('\ninclude!('+json.dumps(str(p/fragment))+');\n').encode())
 result=subprocess.run([sys.executable,str(p/'run.py'),sys.argv[1] if len(sys.argv)>1 else 'adversarial','cargo','test','-p','botstral-manager','--locked','--lib','independent_','--','--nocapture'])
finally:
 for path,original in originals.items(): path.write_bytes(original)
sys.exit(result.returncode)
