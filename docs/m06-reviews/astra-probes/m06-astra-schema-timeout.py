import json
import os
from pathlib import Path
import signal
import subprocess

root = Path(__file__).resolve().parents[1]
build = subprocess.run(['cargo', 'test', '-p', 'botstral-manager', '--lib', '--no-run', '--message-format=json'],cwd=root,capture_output=True,text=True,check=True)
artifacts = [json.loads(line) for line in build.stdout.splitlines()]
binary = next(item['executable'] for item in artifacts if item.get('reason') == 'compiler-artifact' and item.get('executable') and item['target']['name']=='botstral_manager')
command = [binary, '--exact', 'manager::state::connectors::tests::astra_reference_fanout_is_bounded', '--nocapture']
for depth in (0,32):
    print('Schema reference depth',depth,flush=True)
    path = root/f'.audit/m06-astra-schema-depth-{depth}.log'
    with path.open('w') as log:
        process = subprocess.Popen(command,cwd=root,env=dict(os.environ,M06_ASTRA_SCHEMA_DEPTH=str(depth)),stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        try:
            code = process.wait(timeout=10)
            print('exit:',code,flush=True)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL)
            process.wait()
            print('TIMEOUT after 10 seconds inside test; killed only probe process group',flush=True)
    print(path.read_text(),flush=True)
