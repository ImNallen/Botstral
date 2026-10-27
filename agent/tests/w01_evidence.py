#!/usr/bin/env python3
"""Record source equality, production binary hashes and pinned runtime identity."""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess


def sha(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda:f.read(65536),b''):h.update(block)
    return h.hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--remote-source',required=True);p.add_argument('--base',required=True);p.add_argument('--runtime',required=True);p.add_argument('--manager-ssh',default='botstral-manager');p.add_argument('--worker-ssh',default='botstral-computer');p.add_argument('--output',type=Path,required=True)
    args=p.parse_args()
    root=Path(__file__).resolve().parents[2]
    names=subprocess.check_output(['git','ls-files','-co','--exclude-standard'],cwd=root,text=True).splitlines()
    names=sorted(n for n in names if n.endswith(('.rs','.toml','.lock','.mjs','.json','.py','.sh')) and not n.startswith(('agent/tests/evidence/','docs/')))
    hashes={name:sha(root/name) for name in names if (root/name).is_file()}
    source="""import hashlib,json,pathlib,sys,platform
p=json.load(sys.stdin)
def sha(path):
 h=hashlib.sha256()
 with pathlib.Path(path).open('rb') as f:
  for block in iter(lambda:f.read(65536),b''):h.update(block)
 return h.hexdigest()
r={'host':platform.node(),'system':platform.platform(),'binaries':{n:sha(n) for n in p['binaries']}}
if 'source' in p:r['source']={n:sha(pathlib.Path(p['source'])/n) for n in p['names']}
if 'runtime' in p:
 r['runtime_manifest']=sha(pathlib.Path(p['runtime'])/'runtime.json')
 files=json.loads((pathlib.Path(p['runtime'])/'runtime.json').read_text())
 r['runtime_binaries']={n:sha(pathlib.Path(p['runtime'])/n) for n,_ in files if pathlib.Path(n).name in ('node','chrome','chrome-headless-shell')}
print(json.dumps(r))
"""
    def remote(alias,payload):
        return json.loads(subprocess.check_output(['ssh','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes',alias,shlex.join(['python3','-c',source])],input=json.dumps(payload),text=True))
    worker=remote(args.worker_ssh,{'source':args.remote_source,'names':list(hashes),'runtime':args.runtime,'binaries':[args.base+'/bin/production/botstral-agent',args.base+'/bin/production/botstral-manager']})
    manager=remote(args.manager_ssh,{'binaries':[args.base+'/bin/production/botstral-manager']})
    mismatches=[n for n in hashes if hashes[n]!=worker['source'][n]]
    if mismatches:raise SystemExit('Source mismatch: '+', '.join(mismatches))
    if manager['binaries'][args.base+'/bin/production/botstral-manager']!=worker['binaries'][args.base+'/bin/production/botstral-manager']:raise SystemExit('Manager binary mismatch')
    result={'base':'e7404fd956f2d7377d74d28d0dbe26f11ee719d7','head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=root,text=True).strip()),'source':hashes,'worker':worker,'manager':manager,'source_matches':True}
    args.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({'passed':True,'files':len(hashes),'runtime':worker['runtime_manifest'],'output':str(args.output)}))

if __name__=='__main__':main()
