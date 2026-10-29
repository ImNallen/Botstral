import pathlib, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[2]
audit=pathlib.Path(__file__).resolve().parent
target=root/'manager/src/manager/state/runs/tests/publication_tests.rs'
original=target.read_bytes()
try:
    target.write_bytes(original + ('\ninclude!('+__import__('json').dumps(str(audit/'publication-probes.rs'))+');\n').encode())
    result=subprocess.run([sys.executable,str(audit/'run.py'),'adversarial','cargo','test','-p','botstral-manager','--locked','--lib','final_review_','--','--nocapture'],cwd=root)
finally:
    target.write_bytes(original)
sys.exit(result.returncode)
