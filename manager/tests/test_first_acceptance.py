import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock

from first_acceptance import Acceptance
from first_acceptance_remote import dispatch


class Remote:
    host = 'isolated-fixture'

    def __init__(self):
        self.operations = []

    def call(self, operation, **values):
        self.operations.append((operation, values))
        if operation == 'scan_provider':
            return {'verdict': 'NOT SCANNED', 'files': 0, 'leaks': None}
        if operation == 'reconcile_budget':
            return {'cap_microusd': 1000000}
        return {'active': 'inactive'}

    def read(self, path):
        return None


class AcceptanceCleanupTests(unittest.TestCase):
    def fixture(self, output):
        acceptance = Acceptance.__new__(Acceptance)
        acceptance.output = output
        acceptance.socket = acceptance.native = None
        acceptance.manager = Remote()
        acceptance.units = []
        acceptance.report = {'cleanup': []}
        return acceptance

    def test_native_timeout_preserves_remote_shutdown_key_deletion_and_report(self):
        with tempfile.TemporaryDirectory() as directory:
            acceptance = self.fixture(Path(directory))
            acceptance.native = Mock()
            acceptance.native.close.side_effect = RuntimeError('stalled native driver')
            acceptance.native.process.wait.side_effect = subprocess.TimeoutExpired('driver', 15)
            acceptance.units = [(acceptance.manager, 'owned-unit', 'worker.log')]
            acceptance.cleanup()
            self.assertEqual([name for name, _ in acceptance.manager.operations], ['stop', 'scan_provider', 'unlink'])
            self.assertEqual(acceptance.manager.operations[-1][1], {'paths': ['provider-key']})
            saved = json.loads((acceptance.output / 'results.json').read_text())
            self.assertEqual(saved['native_cleanup_error'], 'RuntimeError')
            self.assertEqual(saved['cleanup_errors'], [{'native_termination': 'TimeoutExpired'}])
            self.assertTrue(saved['temporary_provider_key_removed'])

    def test_missing_comparison_key_does_not_claim_a_secret_scan(self):
        with tempfile.TemporaryDirectory() as directory:
            result = dispatch(Path(directory), {'op': 'scan_provider'})
            self.assertEqual(result['verdict'], 'NOT SCANNED')
            self.assertIsNone(result['leaks'])

    def test_budget_evidence_remains_immutable_after_report_update(self):
        with tempfile.TemporaryDirectory() as directory:
            acceptance = self.fixture(Path(directory))
            acceptance.ledger = '/private/campaign/budget.json'
            acceptance.root = '/private/fixture'
            acceptance.run_id = 'fixture'
            acceptance.cleanup()
            snapshot = (acceptance.output / 'budget-evidence.json').read_bytes()
            _, request = acceptance.manager.operations[-1]
            self.assertEqual(request['evidence_sha256'], hashlib.sha256(snapshot).hexdigest())
            self.assertNotEqual(snapshot, (acceptance.output / 'results.json').read_bytes())
            self.assertNotIn('reconciled_budget', json.loads(snapshot))


if __name__ == '__main__':
    unittest.main()
