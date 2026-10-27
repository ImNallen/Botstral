#!/usr/bin/env python3
"""Measure the pinned browser runner in a finite systemd cgroup and check every output."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
import uuid

HTML = b'''<body><input id="login"><button id="save">Save</button><div id="state"></div><script>
const dbp=new Promise((resolve,reject)=>{const r=indexedDB.open('fixture',1);r.onupgradeneeded=()=>r.result.createObjectStore('state');r.onsuccess=()=>resolve(r.result);r.onerror=reject;});
async function ready(){const db=await dbp;const t=db.transaction('state','readonly');const r=t.objectStore('state').get('login');r.onsuccess=()=>{document.querySelector('#state').textContent=[document.cookie,localStorage.getItem('login'),r.result].join('|');document.querySelector('#state').setAttribute('data-ready',r.result==='shared-fixture'?'saved':'empty');};}
document.querySelector('#save').onclick=async()=>{const value=document.querySelector('#login').value;document.cookie='login='+value+'; Max-Age=3600';localStorage.setItem('login',value);const db=await dbp;const t=db.transaction('state','readwrite');t.objectStore('state').put(value,'login');t.oncomplete=()=>ready();};ready();
</script></body>'''

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type','text/html')
        self.end_headers()
        self.wfile.write(HTML)
    def log_message(self,*args):
        pass

def child(args):
    root=Path(args.root)
    cgroup=Path('/sys/fs/cgroup')/next(l[3:] for l in Path('/proc/self/cgroup').read_text().splitlines() if l.startswith('0::')).lstrip('/')
    samples=[]
    results=[]
    for index in range(5):
        work=root/str(index);work.mkdir(mode=0o700)
        outputs=[{'slot':'text','name':'state.txt','media_type':'text_plain','max_bytes':4096},{'slot':'png','name':'state.png','media_type':'png','max_bytes':8388608},{'slot':'file','name':'guest.txt','media_type':'text_plain','max_bytes':4096}]
        operations=[{'type':'navigate','url':args.url},{'type':'fill','selector':'#login','text':'shared-fixture'},{'type':'click','selector':'#save'},{'type':'capture_text','selector':'#state[data-ready="saved"]','output':'text'},{'type':'screenshot','output':'png'},{'type':'write_file','path':str(root/'guest.txt'),'text':'outside workspace\n'},{'type':'read_file','path':str(root/'guest.txt'),'output':'file'}]
        spec={'viewport':{'width':1280,'height':960},'outputs':outputs,'operations':operations}
        (work/'spec.json').write_text(json.dumps(spec))
        begin=time.monotonic()
        p=subprocess.Popen([args.runtime+'/node/bin/node',args.runtime+'/run.mjs','run',str(work/'spec.json'),str(root/'profile'),str(work)],env={'HOME':str(root),'PLAYWRIGHT_BROWSERS_PATH':args.runtime+'/browsers','LANG':'C.UTF-8'},stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        while p.poll() is None:
            samples.append({'memory':int((cgroup/'memory.current').read_text()),'tasks':int((cgroup/'pids.current').read_text())})
            time.sleep(.01)
        stdout,stderr=p.communicate()
        assert p.returncode==0,(p.returncode,stderr)
        text=(work/'text.out').read_text()
        assert text=='login=shared-fixture|shared-fixture|shared-fixture',text
        assert (work/'png.out').read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
        assert (work/'file.out').read_bytes()==b'outside workspace\n'
        if index==0:
            # A later clean context must read all persisted stores without logging in again.
            spec['operations']=[{'type':'navigate','url':args.url},{'type':'capture_text','selector':'#state[data-ready="saved"]','output':'text'}]
            (work/'spec.json').write_text(json.dumps(spec))
            subprocess.run([args.runtime+'/node/bin/node',args.runtime+'/run.mjs','run',str(work/'spec.json'),str(root/'profile'),str(work)],env={'HOME':str(root),'PLAYWRIGHT_BROWSERS_PATH':args.runtime+'/browsers','LANG':'C.UTF-8'},check=True,capture_output=True)
            assert (work/'text.out').read_text()=='login=shared-fixture|shared-fixture|shared-fixture'
        results.append({'run':index,'seconds':time.monotonic()-begin,'outputs':{n:{'length':(work/n).stat().st_size,'sha256':hashlib.sha256((work/n).read_bytes()).hexdigest()} for n in ('text.out','png.out','file.out')}})
    result={'passed':True,'runs':results,'memory_peak':int((cgroup/'memory.peak').read_text()),'task_peak_sampled':max(v['tasks'] for v in samples),'memory_max':(cgroup/'memory.max').read_text().strip(),'tasks_max':(cgroup/'pids.max').read_text().strip(),'samples':samples,'runtime_identity':hashlib.sha256(Path(args.runtime,'runtime.json').read_bytes()).hexdigest(),'node':subprocess.check_output([args.runtime+'/node/bin/node','--version'],text=True).strip(),'machine':os.uname()._asdict() if hasattr(os.uname(),'_asdict') else list(os.uname())}
    try:result['task_peak']=int((cgroup/'pids.peak').read_text())
    except FileNotFoundError:pass
    (root/'measurements.json').write_text(json.dumps(result,indent=2))
    print(json.dumps({k:v for k,v in result.items() if k!='samples'}))

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runtime',required=True);p.add_argument('--root',required=True);p.add_argument('--child',action='store_true');p.add_argument('--url')
    args=p.parse_args()
    if args.child:return child(args)
    os.umask(0o077)
    Path(args.root).mkdir(mode=0o700)
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    unit='w01-measure-'+str(uuid.uuid4())+'.service'
    try:
        subprocess.run(['systemd-run','--user','--wait','--pipe','--unit='+unit,'--property=MemoryMax=1024M','--property=MemorySwapMax=0','--property=TasksMax=128','--property=RuntimeMaxSec=180','--property=KillMode=control-group','--property=TimeoutStopSec=2','/usr/bin/python3',str(Path(__file__).resolve()),'--child','--runtime',args.runtime,'--root',args.root,'--url',f'http://127.0.0.1:{server.server_port}/'],check=True)
    finally:
        server.shutdown()
        subprocess.run(['systemctl','--user','stop',unit],capture_output=True)
        state=subprocess.run(['systemctl','--user','show',unit,'--property=LoadState,ActiveState,ControlGroup'],capture_output=True,text=True)
        Path(args.root,'cleanup.json').write_text(json.dumps({'unit':unit,'after':state.stdout}))

if __name__=='__main__':main()
