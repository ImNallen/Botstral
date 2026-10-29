import json, os, pathlib, subprocess, sys, time
root = pathlib.Path(__file__).resolve().parents[2]
out = pathlib.Path(__file__).resolve().parent
label, *cmd = sys.argv[1:]
started = time.time()
with (out / (label + '.log')).open('w') as log:
    p = subprocess.run(cmd, cwd=root, stdout=log, stderr=subprocess.STDOUT)
result = dict(command=cmd, cwd=str(root), exit=p.returncode, elapsed_seconds=time.time()-started)
(out / (label + '.json')).write_text(json.dumps(result, indent=2)+'\n')
print(json.dumps(result))
print((out / (label + '.log')).read_text()[-5000:])
sys.exit(p.returncode)
