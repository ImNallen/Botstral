#!/usr/bin/env python3
"""Verify production process effects and recovery across two existing SSH hosts."""
import argparse
import json
import os
from pathlib import Path
import sys
import time
import uuid
import m02_two_vm as m02


class Remote(m02.Remote):
    def install(self):
        sources = {name: Path(__file__).with_name(name).read_text()
                   for name in ("m02_remote.py", "w01_remote.py")}
        bootstrap = (
            "import os,json,pathlib,sys; os.umask(0o077); p=json.load(sys.stdin); "
            "r=pathlib.Path(p['root']); r.mkdir(mode=0o700,parents=True,exist_ok=False); "
            "[(r/n).write_text(s) for n,s in p['sources'].items()]; "
            "print(json.dumps({'ok':True,'value':True}))"
        )
        self.ssh(["python3", "-c", bootstrap], {"root": self.root, "sources": sources})

    def call(self, op, **values):
        return self.ssh(["python3", self.root + "/w01_remote.py"], {"op": op, **values})


def request(f, session, args):
    deadline = int(time.time()) + 120
    return {"id": str(uuid.uuid4()), "session_id": session["id"], "conversation_id": None,
            "settings": {"model": "unavailable", "max_turns": 1, "max_tokens": 1,
                         "deadline": deadline, "max_cost_micros": 0},
            "work": {"type": "process", "spec": {"profile": "linux_systemd_v1",
                "account": {"uid": 1000, "gid": 1000}, "executable": "/usr/bin/python3",
                "args": args, "environment": {"LITERAL": "$HOME %n", "LC_ALL": "C"}, "cwd": f.worker.root,
                "deadline": deadline, "limits": limits()}}}


def limits():
    return {"runtime_seconds": 60, "output_bytes": 1024, "processes": 32, "memory_bytes": 268435456}


def task(f, original, state):
    return f.poll(lambda: f.owner("GET", "/v1/tasks/" + original["id"]),
                  lambda v: v["state"] == state, "task " + state)


def approve(f, original):
    return f.owner("PUT", "/v1/tasks/" + original["id"] + "/approval",
                   {"expires_at": int(time.time()) + 90})


def cancel(f, original):
    f.owner("POST", "/v1/tasks/" + original["id"] + "/cancel")
    return task(f, original, "cancelled")


def descendants(f, session, name):
    path = f.worker.root + "/" + name + ".py"
    source = """import os,signal,time
signal.signal(signal.SIGTERM,signal.SIG_IGN)
p=os.getpid()
c=os.fork()
if c==0:
    os.setsid()
    while True: time.sleep(.1)
with open(__file__+'.ledger','a') as f: f.write('one\\n')
with open(__file__+'.pids','w') as f: f.write(str(p)+' '+str(c))
print('observed effect',flush=True)
while True: time.sleep(.1)
"""
    f.worker.call("write", files={path: source})
    original = f.owner("POST", "/v1/tasks", request(f, session, [path]))
    approve(f, original)
    text = f.poll(lambda: f.worker.call("read", path=path + ".pids", optional=True),
                  lambda v: v is not None, "guest process identities")
    pids = [int(v) for v in text.split()]
    before = f.worker.call("liveness", pids=pids)
    m02.require(all(v["live"] for v in before), "both processes must be live")
    return original, pids, path, before


def restart_manager(f):
    old = next(p for p in f.processes if p.remote is f.manager and not p.stopped)
    f.stop(old)
    f.manager.call("remove_listeners", root=f.manager.root)
    certs = f.manager.root + "/certs/"
    f.launch(f.manager, "manager-restarted", [m02.BASE + "/bin/production/botstral-manager",
        "--state-dir", f.manager.root + "/state", "--listen", f.listen, "--setup-listen", "127.0.0.1:0",
        "--worker-listen", f.address, "--worker-tls-cert", certs + "manager.pem",
        "--worker-tls-key", certs + "manager.key", "--worker-client-ca", certs + "ca.pem"])
    f.poll(lambda: f.manager.call("read", path=f.manager.root + "/state/listeners.json", optional=True),
           lambda v: v is not None, "restarted listeners")
    login = f.owner("POST", "/v1/sessions", {"password": m02.PASSWORD}, status=201, token="")
    f.owner_token = login["token"]
    f.secrets.append(f.owner_token)


def exercise(f, certificates):
    with f.check("production_enrollment"):
        f.enroll(f.invitation(certificates), certificates)
        session = f.session("W01 process")
        f.worker.call("write", files={f.worker.root + "/agent/process.json": json.dumps({
            "profile": "linux_systemd_v1", "account": {"uid": 1000, "gid": 1000}, "ceilings": limits()})})
        f.start_daemon("daemon")
        session = f.confirmed(session)
    with f.check("approved_exact_effect_and_duplicate") as details:
        path = f.worker.root + "/success.py"
        source = """import os,sys,json
with open('invocations','a') as f: f.write('one\\n')
with open('effect.json','w') as f: json.dump({'args':sys.argv[1:],'cwd':os.getcwd(),'env':dict(os.environ)},f)
os.write(1,b'stdout\\x00\\xff')
os.write(2,b'stderr')
"""
        f.worker.call("write", files={path: source})
        submitted = request(f, session, [path, "$HOME", "%n", "a b"])
        original = f.owner("POST", "/v1/tasks", submitted)
        task(f, original, "awaiting_approval")
        m02.require(f.worker.call("read", path=f.worker.root + "/invocations", optional=True) is None,
                    "command ran before approval")
        changed = json.loads(json.dumps(submitted))
        changed["work"]["spec"]["args"] = ["changed"]
        f.owner("POST", "/v1/tasks", changed, status=409)
        approve(f, original)
        result = task(f, original, "completed")
        effect = json.loads(f.worker.call("read", path=f.worker.root + "/effect.json"))
        m02.require(effect["args"] == ["$HOME", "%n", "a b"] and effect["cwd"] == f.worker.root,
                    "arguments or working directory changed")
        m02.require(effect["env"] == {"LITERAL": "$HOME %n", "LC_ALL": "C"},
                    "environment was inherited or expanded")
        m02.require(bytes.fromhex(result["process"]["result"]["stdout"]["hex"]) == b"stdout\x00\xff",
                    "stdout bytes changed")
        m02.require(bytes.fromhex(result["process"]["result"]["stderr"]["hex"]) == b"stderr", "stderr changed")
        m02.require(f.owner("POST", "/v1/tasks", submitted)["command"] == original["command"], "duplicate command identity changed")
        m02.require(f.worker.call("read", path=f.worker.root + "/invocations") == "one\n", "duplicate launch")
        details.update(task=result, effect=effect, invocation_count=1)
    with f.check("responsive_descendant_cancellation") as details:
        original, pids, path, before = descendants(f, session, "cancel")
        began = time.monotonic()
        f.workers()
        elapsed = time.monotonic() - began
        m02.require(elapsed < 2, "manager request blocked behind command")
        time.sleep(11)
        result = cancel(f, original)
        after = f.worker.call("liveness", pids=pids)
        m02.require(not any(p["live"] for p in after), "cancelled while descendant live")
        m02.require(result["process"]["effect"] == "observed", "cancellation discarded effect evidence")
        details.update(before=before, after=after, request_seconds=elapsed, task=result)
    for name in ("manager-restart", "daemon-crash"):
        with f.check(name) as details:
            original, pids, path, before = descendants(f, session, name)
            if name == "manager-restart":
                restart_manager(f)
            else:
                f.worker.call("kill", unit=f.daemon.unit)
                f.daemon.stopped = True
                f.start_daemon("daemon-restarted")
            result = task(f, original, "awaiting_attention")
            after = f.worker.call("liveness", pids=pids)
            m02.require(not any(p["live"] for p in after), "recovery did not stop descendants")
            m02.require(result["command"] == original["command"], "recovery replaced execution identity")
            m02.require(f.worker.call("read", path=path + ".ledger") == "one\n", "recovery repeated effects")
            cancelled = cancel(f, original)
            m02.require(cancelled["process"] == result["process"], "later cancellation erased evidence")
            details.update(before=before, after=after, task=cancelled, invocation_count=1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manager-ssh", default="botstral-manager")
    parser.add_argument("--worker-ssh", default="botstral-computer")
    parser.add_argument("--manager-address", default="192.168.1.40:43123")
    parser.add_argument("--base", required=True)
    parser.add_argument("--cert-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    m02.BASE = args.base
    output = args.output_dir / str(uuid.uuid4())
    output.mkdir(mode=0o700, parents=True)
    names = ["ca.pem", "manager.pem", "manager.key", "worker.pem", "worker.key"]
    certificates = {n: (args.cert_dir / n).read_text() for n in names}
    root = args.base + "/runs/" + output.name
    f = m02.Fixture("production", Remote(args.manager_ssh, root), Remote(args.worker_ssh, root),
                    args.manager_address, output, [m02.PASSWORD, m02.VAULT, *certificates.values()])
    try:
        m02.require(args.manager_ssh != args.worker_ssh, "separate SSH hosts required")
        m02.setup(f, certificates)
        for remote, name in [(f.manager, "manager"), (f.worker, "computer")]:
            m02.record(name + ".versions", "passed", remote.call("versions"))
        exercise(f, certificates)
    except Exception as error:
        m02.record("fixture", "failed", str(error) if isinstance(error, m02.CheckFailure) else type(error).__name__)
    finally:
        f.finish()
        try:
            m02.record("process_cleanup", "passed", f.worker.call("cleanup_processes", root=root, units=[p.unit for p in f.processes if p.remote is f.worker]))
        except Exception as error:
            m02.record("process_cleanup", "failed", type(error).__name__)
    required = {"production." + n for n in ["approved_exact_effect_and_duplicate", "responsive_descendant_cancellation", "manager-restart", "daemon-crash"]}
    missing = required - {v["name"] for v in m02.RESULTS if v["status"] == "passed"}
    passed = not missing and all(v["status"] == "passed" for v in m02.RESULTS)
    m02.private_json(output / "results.json", {"passed": passed, "checks": m02.RESULTS, "root": root})
    print(json.dumps({"passed": passed, "evidence": str(output.resolve())}))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
