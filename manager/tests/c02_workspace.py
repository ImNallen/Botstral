#!/usr/bin/env python3
"""Actual-manager Workspace and reviewed approval boundary contract."""
import argparse
import copy
import json
import os
from pathlib import Path
import shutil
import sqlite3
import tempfile
import time
import uuid
import m05_acceptance as m


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binaries', type=Path, default=Path('target/debug'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--old-probe', type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    args.output.mkdir(parents=True, exist_ok=False)
    root = Path(tempfile.mkdtemp(prefix='botstral-c02-workspace-'))
    fixture = None
    try:
        m.certificates(root)
        fixture = m.Fixture(root, args.binaries/'botstral-manager', args.binaries/'owner-probe', args.binaries/'botstral-agent')
        fixture.start()
        fixture.setup()
        client = fixture.client('owner')
        bot, worker, session = [str(uuid.uuid4()) for _ in range(3)]
        fixture.owner('PUT', '/v1/bots/'+bot, {'name': 'Workspace contract'})
        with sqlite3.connect(root/'state/manager.sqlite3') as db:
            db.execute('INSERT INTO workers(id,fingerprint) VALUES(?,?)', (worker, 'a'*64))
            db.execute("INSERT INTO computer_sessions VALUES(?,?,?,'bot',1)", (session, bot, worker))
        fixture.owner('PUT', '/v1/bots/'+bot+'/permission', {'expected_revision': 1, 'allow': True})
        image = fixture.call(client, {'method': 'workspace'})['value']
        m.require(image['bots'][0]['id'] == bot and image['sessions'][0]['id'] == session, 'Workspace omitted the authoritative roster or session')
        replay = fixture.call(client, {'method': 'resume', 'cursor': 0, 'limit': 64})['value']
        m.require(set(replay['state']) == {'tasks','inferences','runs','inference_configurations','conversations','messages','documents','permissions'}, 'Legacy Resume shape changed')
        if args.old_probe:
            operation = root/'old-resume.json'
            m.write(operation, {'method': 'resume', 'cursor': 0, 'limit': 64})
            old = json.loads(m.command([args.old_probe, 'call', client, operation]).stdout)
            m.require(old['type'] == 'replay', 'Old client failed against new manager')
        task = fixture.call(client, {'method': 'submit_task', 'input': {'id': str(uuid.uuid4()), 'session_id': session, 'conversation_id': None, 'settings': {'model': 'fixture', 'max_turns': 2, 'max_tokens': 100, 'deadline': int(time.time())+300, 'max_cost_micros': 100}, 'work': {'type': 'process', 'spec': {'profile': 'linux_systemd_v1', 'account': {'uid': 1000, 'gid': 1000}, 'executable': '/usr/bin/printf', 'args': ['reviewed'], 'environment': {}, 'cwd': '/tmp', 'deadline': int(time.time())+300, 'limits': {'runtime_seconds': 20, 'output_bytes': 1024, 'processes': 8, 'memory_bytes': 67108864}}}}})['value']
        # Normalize the two separately sampled deadlines before submitting if the clock ticks.
        review = {'method': 'grant_reviewed_approval', 'id': task['id'], 'command': task['command'], 'approval': task['approval']['id'], 'permission_revision': 2, 'expires_at': int(time.time())+60}
        wrong = copy.deepcopy(review)
        wrong['command']['work']['spec']['args'] = ['different action']
        fixture.call(client, wrong, success=False)
        m.require(fixture.call(client, {'method': 'task', 'id': task['id']})['value']['approval']['binding'] is None, 'Changed action acquired approval')
        fixture.owner('PUT', '/v1/bots/'+bot+'/permission', {'expected_revision': 2, 'allow': True})
        fixture.call(client, review, success=False)
        review['permission_revision'] = 3
        granted = fixture.call(client, review)['value']
        m.require(granted['approval']['binding']['permission_revision'] == 3 and granted['approval']['binding']['execution'] == task['command']['execution'], 'Reviewed grant did not bind exact execution')
        checks = ['consistent_workspace', 'legacy_resume_shape', 'changed_action_refused', 'stale_permission_refused', 'exact_execution_granted']
        m.write(args.output/'summary.json', {'passed': True, 'checks': checks, 'old_probe': bool(args.old_probe)})
        print(json.dumps({'passed': True, 'checks': checks}))
    finally:
        if fixture:
            fixture.stop()
        shutil.rmtree(root)

if __name__ == '__main__':
    main()
