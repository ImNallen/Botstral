import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT=Path(__file__).resolve().parents[1]
os.umask(0o077)
with tempfile.TemporaryDirectory(dir=ROOT/'.audit') as temporary:
    state=Path(temporary).resolve()
    configuration=subprocess.check_output([sys.executable, str(ROOT/'agent/connector/configure.py'), '--python', sys.executable], cwd=ROOT)
    (state/'connector.json').write_bytes(configuration)
    print('Generated ceilings:',json.loads(configuration)['ceilings'])
    result=subprocess.run(['cargo','test','-p','botstral-agent','--lib','astra_connector_configuration_probe','--','--nocapture','--ignored'],cwd=ROOT,env=dict(os.environ,M06_ASTRA_STATE=str(state)),text=True,capture_output=True,check=True)
    for line in (result.stdout+result.stderr).splitlines():
        if line.startswith('Production worker configuration'):
            print(line)
