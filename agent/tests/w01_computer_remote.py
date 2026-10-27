#!/usr/bin/env python3
import os
os.umask(0o077)
import hashlib
import http.client
import json
from pathlib import Path
import sqlite3
import sys
import w01_remote


def dispatch(r):
    op=r['op']
    if op=='download':
        host,port=r['address'].rsplit(':',1)
        assert host=='127.0.0.1'
        c=http.client.HTTPConnection(host,int(port),timeout=10)
        try:
            c.request('GET',r['path'],headers={'Authorization':'Bearer '+r.get('token','')})
            response=c.getresponse();data=response.read(8388609)
            assert len(data)<=8388608
            return {'status':response.status,'headers':dict(response.getheaders()),'length':len(data),'sha256':hashlib.sha256(data).hexdigest(),'prefix':data[:100].hex()}
        finally:c.close()
    if op=='database':
        db=sqlite3.connect('file:'+r['path']+'?mode=ro',uri=True)
        try:
            assert r['query'] in ('SELECT id,slot,offset,state,reserved FROM artifacts','SELECT record FROM executions')
            return db.execute(r['query']).fetchall()
        finally:db.close()
    if op=='scopes':
        scopes=[]
        for p in Path(r['root'],'agent','processes').glob('*/ready.json'):
            ready=json.loads(p.read_text());scope=ready['scope'];cg=Path('/sys/fs/cgroup')/scope['cgroup'].lstrip('/')
            values={}
            for name in ('memory.peak','memory.max','pids.peak','pids.current','cgroup.events','cgroup.procs'):
                try:values[name]=(cg/name).read_text().strip()
                except FileNotFoundError:values[name]=None
            scopes.append({'scope':scope,'values':values})
        return scopes
    if op=='remove_snapshot':
        import uuid
        execution=str(uuid.UUID(r['execution']))
        slot=r['slot'];assert slot.isalnum()
        Path(r['root'],'agent','spool',execution+'-'+slot+'.blob').unlink()
        return True
    if op=='corrupt_profile':
        path=Path(r['root'],'agent','automation-profile','Default','Cookies')
        path.rename(path.with_suffix('.fixture-backup'))
        with path.open('x') as f:f.write('corrupt cookie database');f.flush();os.fsync(f.fileno())
        return True
    if op=='move':
        Path(r['source']).rename(r['destination']);return True
    if op in ('corrupt','corrupt_partial'):
        import uuid
        identity=str(uuid.UUID(r['id']))
        path=Path(r['root'],'state','artifacts',identity+('.part' if op=='corrupt_partial' else '.blob'))
        assert path.is_file()
        with path.open('r+b') as f:f.write(b'corrupt');f.flush();os.fsync(f.fileno())
        return True
    return w01_remote.dispatch(r)

if __name__=='__main__':
    try:print(json.dumps({'ok':True,'value':dispatch(json.load(sys.stdin))}))
    except Exception as e:
        print(json.dumps({'ok':False,'error':type(e).__name__}));sys.exit(1)
