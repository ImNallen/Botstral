import pathlib,subprocess,sys,json
root=pathlib.Path.cwd(); p=pathlib.Path(__file__).resolve().parent
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()=='086b1acb41226be3e5ace6055be6f765c070b8fa'
subprocess.run(['git','diff','--exit-code','--quiet'],check=True)
pairs=[('manager/src/manager/state/runs/tests/publication_tests.rs','publication-probes.rs'),('manager/src/manager/state/credentials/tests/live/authority.rs','generated-probes.rs')]
originals={root/target:(root/target).read_bytes() for target,_ in pairs}
try:
 for target,fragment in pairs:
  path=root/target; path.write_bytes(originals[path]+('\ninclude!('+json.dumps(str(p/fragment))+');\n').encode())
 result=subprocess.run([sys.executable,str(p/'run.py'),sys.argv[1] if len(sys.argv)>1 else 'adversarial','cargo','test','-p','botstral-manager','--locked','--lib',sys.argv[2] if len(sys.argv)>2 else 'fresh_catalog_','--','--nocapture'])
finally:
 for path,original in originals.items(): path.write_bytes(original)
sys.exit(result.returncode)
