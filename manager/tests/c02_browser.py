#!/usr/bin/env python3
"""Recheck native composer browser execution against a retained C02 fixture."""
import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

from c02_production import Native, Remote, ROOT, m, poll, private


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--native', type=Path, default=ROOT/'desktop/src-tauri/target/debug/examples/c02-remote-driver')
    args = parser.parse_args()
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    control = json.loads(args.fixture.read_text())
    manager = Remote(control['manager_alias'], control['root'])
    computer = Remote(control['computer_alias'], control['root'])
    fixture = json.loads(manager.call('read', path='fixture.json'))
    response = manager.call('http', address=fixture['local'], method='POST', path='/v1/sessions', body=dict(password=fixture['password']))
    m.require(response['status'] == 201, 'fixture login')
    token = response['body']['token']
    record = dict(status='BLOCKED', native_remote_source_sha256=hashlib.sha256((ROOT/'desktop/src-tauri/src/remote.rs').read_bytes()).hexdigest(), native_source_dirty=bool(subprocess.check_output(['git', 'status', '--porcelain', '--', 'desktop/src-tauri/src/remote.rs'], cwd=ROOT, text=True).strip()), native_source=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), manager_source=control['source_sha'], native_sha256=hashlib.sha256(args.native.read_bytes()).hexdigest(), endpoint=control['trust']['endpoint'], leaf=control['trust']['leaf'])
    native = None
    local = Path(tempfile.mkdtemp(prefix='c02-browser-private-'))
    try:
        native = Native(args.native, local)
        request = native.call('begin')['view']['pairing']
        response = manager.call('http', address=fixture['local'], method='POST', path='/v1/device-invitations', token=token, body=dict(request=request, lifetime_seconds=600))
        m.require(response['status'] == 200, 'fixture invitation')
        grant = response['body']
        private(local/'invitation.json', dict(trust=control['trust'], **{key: grant[key] for key in ('role', 'fingerprint', 'expires_at', 'invitation')}))
        candidate = native.call('stage', file=str(local/'invitation.json'))['view']['candidate']
        native.call('confirm', id=candidate['id'])
        prompt = 'C02 browser native policy '+args.output.name
        native.call('send', input=dict(bot=control['bot'], session=control['session'], text=prompt, account=dict(uid=1000, gid=1000), runtime=control['runtime'], process=True, seconds=120, turns=4, tokens=200000, cost_microusd=200000, output_tokens=1024))

        def snapshot(predicate, label):
            return poll(lambda: native.call('inspect'), lambda x: x['workspace'] is not None and predicate(x['workspace']), label, 150)['workspace']

        workspace = snapshot(lambda w: any(r['request']['prompt'] == prompt and any(t.get('intent') for t in r['turns']) for r in w['runs']), 'native browser proposal')
        run = next(r for r in workspace['runs'] if r['request']['prompt'] == prompt)
        task_id = next(t['intent']['task'] for t in run['turns'] if t.get('intent'))
        review = native.call('review', id=task_id)['view']['review']
        native.call('approve', id=review['id'])
        workspace = snapshot(lambda w: any(t['id'] == task_id and t['state'] in ('completed', 'failed', 'cancelled') and all(o['state']['type'] in ('available', 'unavailable') for o in t['outputs']) for t in w['state']['tasks']), 'native browser completion')
        task = next(t for t in workspace['state']['tasks'] if t['id'] == task_id)
        record.update(run=run['request'], task=task)
        m.require(task['state'] == 'completed', 'native composer browser failed')
        for slot, name in [('text', 'browser-export.txt'), ('image', 'browser.png')]:
            output = next(o for o in task['outputs'] if o['declaration']['slot'] == slot)
            m.require(output['state']['type'] == 'available', 'native browser artifact unavailable')
            native.call('export', task=task_id, artifact=output['state']['id'], name=name)
            shutil.copy2(local/name, args.output/name)
        m.require((args.output/'browser-export.txt').read_bytes() == b'C02 production browser proof', 'actual DOM result')
        m.require((args.output/'browser.png').read_bytes().startswith(b'\x89PNG\r\n\x1a\n'), 'actual browser PNG')
        m.require(computer.call('read', path='browser.txt') == 'C02 production file proof\n', 'actual guest file')
        record.update(status='PASS', artifacts={name: hashlib.sha256((args.output/name).read_bytes()).hexdigest() for name in ['browser-export.txt', 'browser.png']})
        native.close()
        native = None
    except Exception as error:
        record.update(status='ISSUES', error=str(error))
    finally:
        if native:
            native.close()
        shutil.rmtree(local)
        private(args.output/'results.json', record)
    print(record['status']+' native composer browser, actual DOM/file effects and verified exports'+(': '+record['error'] if 'error' in record else ''))
    return 0 if record['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
