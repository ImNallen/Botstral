#!/usr/bin/env python3
"""Check browser acceptance cleanup with a local protocol subprocess."""
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

import c02_browser as browser
import c02_production as production


class Remote:
    def __init__(self, *args):
        pass

    def call(self, op, **values):
        if op == 'read':
            return json.dumps({'local': '127.0.0.1:1', 'password': 'synthetic'})
        if values['path'] == '/v1/sessions':
            return {'status': 201, 'body': {'token': 'synthetic'}}
        return {'status': 200, 'body': dict(role='owner', fingerprint='0'*64,
                expires_at=0, invitation='synthetic-review-invitation')}


class Native(production.Native):
    fail_cleanup = False
    instances = []

    def __init__(self, binary, root):
        super().__init__(binary, root)
        self.instances.append(self)

    def call(self, op, **values):
        if op == 'quit':
            if self.fail_cleanup:
                with patch.object(production.select, 'select', return_value=([], [], [])):
                    return super().call(op, **values)
            return super().call(op, **values)
        if op == 'begin':
            return {'view': {'pairing': {'fingerprint': '0'*64}}}
        if op == 'stage':
            return {'view': {'candidate': {'id': 'synthetic'}}}
        if op == 'confirm':
            return {}
        raise RuntimeError('injected send transport failure')


class BrowserCleanup(unittest.TestCase):
    def test_send_failure_cleans_private_files(self):
        self.check_cleanup(False, False)

    def test_shutdown_timeout_preserves_result_and_cleans_private_files(self):
        self.check_cleanup(True, False)

    def test_file_cleanup_failure_preserves_result(self):
        self.check_cleanup(False, True)

    def check_cleanup(self, shutdown_timeout, file_failure):
        with tempfile.TemporaryDirectory(prefix='c02-browser-cleanup-') as temporary:
            root = Path(temporary)
            output, local = root/'output', root/'private'
            local.mkdir(mode=0o700)
            fixture = root/'fixture.json'
            fixture.write_text(json.dumps(dict(manager_alias='local-fake', computer_alias='local-fake', root='synthetic', source_sha='synthetic', trust=dict(endpoint='synthetic', leaf='0'*64), bot='synthetic', session='synthetic', runtime='0'*64)))
            binary = root/'local-protocol-fixture'
            binary.write_text("#!/usr/bin/env python3\nimport json,sys\nfor line in sys.stdin:\n    print(json.dumps({'ok':True,'error':None}),flush=True)\n    break\n")
            binary.chmod(0o700)
            Native.fail_cleanup = shutdown_timeout
            Native.instances = []
            remove = shutil.rmtree

            def remove_private(path):
                if file_failure:
                    raise OSError('injected private removal failure')
                remove(path)

            try:
                with patch.object(browser, 'Remote', Remote), patch.object(browser, 'Native', Native), patch.object(browser.tempfile, 'mkdtemp', return_value=str(local)), patch.object(browser.shutil, 'rmtree', side_effect=remove_private), patch.object(sys, 'argv', ['c02_browser.py', '--fixture', str(fixture), '--output', str(output), '--native', str(binary)]):
                    result = browser.main()
                self.assertEqual(result, 1)
                record = json.loads((output/'results.json').read_text())
                self.assertEqual(record['status'], 'ISSUES')
                self.assertEqual(record['error'], 'injected send transport failure')
                self.assertEqual('native_shutdown_error' in record, shutdown_timeout)
                self.assertEqual('local_cleanup_error' in record, file_failure)
                self.assertEqual(local.exists(), file_failure)
                self.assertEqual((local/'invitation.json').exists(), file_failure)
                for native in Native.instances:
                    self.assertIsNotNone(native.process.poll())
            finally:
                for native in Native.instances:
                    if native.process.poll() is None:
                        native.process.kill()
                    native.process.wait(timeout=5)
                    native.process.stdin.close()
                    native.process.stdout.close()


if __name__ == '__main__':
    unittest.main()
