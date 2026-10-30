import pathlib,sys,subprocess,json,datetime
p=pathlib.Path(__file__).resolve().parent
label,*cmd=sys.argv[1:]
start=datetime.datetime.now(datetime.timezone.utc).isoformat()
with (p/(label+'.log')).open('w') as out:
 r=subprocess.run(cmd,stdout=out,stderr=subprocess.STDOUT)
record=dict(command=cmd,cwd=str(pathlib.Path.cwd()),exit=r.returncode,start=start,end=datetime.datetime.now(datetime.timezone.utc).isoformat())
(p/(label+'.json')).write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps(record));print((p/(label+'.log')).read_text()[-7000:])
sys.exit(r.returncode)
