#!/usr/bin/env python3
"""Archive frozen production source plus explicit test support and build on Computer."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import shlex
import subprocess
import tarfile
import uuid

SOURCE = 'e81cdbd51e2e852d2e6a78b5505eccf3a43ff8ed'
OVERLAYS = ['manager/src/manager/inference/tests.rs', 'manager/src/manager/inference/run_tests.rs', 'manager/src/manager/inference/production_fixture.rs']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--computer', default='botstral-computer')
    args = parser.parse_args()
    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    original = subprocess.check_output(['git', 'archive', '--format=tar', SOURCE])
    buffer = io.BytesIO()
    with tarfile.open(fileobj=io.BytesIO(original)) as source, tarfile.open(fileobj=buffer, mode='w') as target:
        for item in source:
            if item.name not in OVERLAYS:
                target.addfile(item, source.extractfile(item) if item.isfile() else None)
        for name in OVERLAYS:
            data = Path(name).read_bytes()
            item = tarfile.TarInfo(name)
            item.size, item.mode = len(data), 0o644
            target.addfile(item, io.BytesIO(data))
    archive = args.output/'source.tar.gz'
    archive.write_bytes(gzip.compress(buffer.getvalue(), mtime=0))
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    root = '/home/botstral/c02-build-'+uuid.uuid4().hex[:12]
    record = dict(source_sha=SOURCE, overlays={n: hashlib.sha256(Path(n).read_bytes()).hexdigest() for n in OVERLAYS}, archive_sha256=digest, build_root=root)
    (args.output/'build.json').write_text(json.dumps(record, indent=2)+'\n')
    subprocess.run(['ssh', args.computer, shlex.join(['mkdir', '-m', '700', root])], check=True)
    subprocess.run(['scp', str(archive), args.computer+':'+root+'/source.tar.gz'], check=True)
    script = '''import hashlib,json,pathlib,subprocess,sys,tarfile
r=pathlib.Path(sys.argv[1]); expected=sys.argv[2]
assert hashlib.sha256((r/'source.tar.gz').read_bytes()).hexdigest()==expected
with tarfile.open(r/'source.tar.gz') as t: t.extractall(r/'source',filter='data')
with (r/'build.log').open('w') as log:
 p=subprocess.run(['/home/botstral/.cargo/bin/cargo','test','-p','botstral-manager','--locked','--lib','--no-run','--message-format=json'],cwd=r/'source',stdout=subprocess.PIPE,stderr=log,text=True)
 artifacts=[json.loads(x) for x in p.stdout.splitlines() if x.startswith('{')]
 for x in artifacts:
  if x.get('reason')=='compiler-message': log.write(x['message'].get('rendered',''))
 binaries=[x['executable'] for x in artifacts if x.get('reason')=='compiler-artifact' and x.get('executable') and x.get('profile',{}).get('test')]
assert p.returncode==0, 'Linux build failed. Inspect '+str(r/'build.log')
assert len(binaries)==1,binaries
pathlib.Path(binaries[0]).rename(r/'manager-fixture')
print(json.dumps({'binary':str(r/'manager-fixture'),'sha256':hashlib.sha256((r/'manager-fixture').read_bytes()).hexdigest(),'exit':p.returncode}))
'''
    result = subprocess.run(['ssh', args.computer, shlex.join(['python3', '-c', script, root, digest])], capture_output=True, text=True)
    record['command'] = 'cargo test -p botstral-manager --locked --lib --no-run --message-format=json'
    record['exit'] = result.returncode
    if result.returncode == 0:
        record['artifact'] = json.loads(result.stdout)
    else:
        record['error'] = result.stderr
    (args.output/'build.json').write_text(json.dumps(record, indent=2)+'\n')
    subprocess.run(['scp', args.computer+':'+root+'/build.log', str(args.output/'linux-build.log')], check=True)
    print(json.dumps(record))
    return result.returncode


if __name__ == '__main__':
    raise SystemExit(main())
