from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import fcntl
import json
import os
import sqlite3
import subprocess
import tempfile
import time
import unittest
import uuid

from m04_remote import acceptance_budget
from m04_two_vm import CALL_HOLD, CALL_PLAN
from m04_reconcile_budget import reconcile


class AcceptanceBudgetTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.request = {
            'path': str(Path(self.directory.name).resolve() / 'budget.json'),
            'run': str(uuid.uuid4()),
            'cap_microusd': 1000000,
            'previous_reserved_microusd': 8 * 54068,
            'reservation_microusd': CALL_HOLD * sum(CALL_PLAN.values()),
        }

    def test_retries_keep_the_full_hold_and_cannot_create_an_extra_run(self):
        ledger = acceptance_budget(self.request)
        self.assertEqual(acceptance_budget(self.request), ledger)
        with self.assertRaises(ValueError):
            acceptance_budget(self.request | {'run': str(uuid.uuid4())})
        with self.assertRaises(ValueError):
            acceptance_budget(self.request | {'previous_reserved_microusd': 0})
        self.assertEqual(acceptance_budget(self.request), ledger)

    def test_concurrent_runs_cannot_overcommit_the_cap(self):
        def attempt(_):
            try:
                acceptance_budget(self.request | {'run': str(uuid.uuid4())})
                return True
            except ValueError:
                return False
        with ThreadPoolExecutor(max_workers=4) as pool:
            self.assertEqual(sum(pool.map(attempt, range(8))), 1)

    def test_symlink_ledger_is_refused(self):
        path = Path(self.request['path'])
        path.symlink_to(path.with_name('missing'))
        with self.assertRaises(ValueError):
            acceptance_budget(self.request)
        self.assertFalse(path.with_name('missing').exists())

    def fixture_state(self):
        root = Path(self.directory.name) / self.request['run']
        state = root / 'state'
        state.mkdir(parents=True)
        (state / 'manager.lock').touch(mode=0o600)
        database = state / 'manager.sqlite3'
        connection = sqlite3.connect(database)
        for table in ('run_turns', 'coding_calls', 'inferences'):
            connection.execute('CREATE TABLE ' + table + '(view TEXT)')
        for table, phase in [('run_turns', {'type':'responded'}), ('coding_calls', 'unknown')]:
            connection.execute('INSERT INTO ' + table + ' VALUES(?)', (json.dumps({
                'id':str(uuid.uuid4()), 'reservation':{'cost_microusd':CALL_HOLD}, 'phase':phase}),))
        connection.commit()
        connection.close()
        return root

    def test_reconciliation_retains_settled_and_unknown_attempt_ceilings(self):
        before = acceptance_budget(self.request)
        request = {'root':str(self.fixture_state()), 'run':self.request['run'],
                   'ledger':self.request['path'], 'evidence_sha256':'fixture-proof'}
        after = reconcile(request)
        self.assertEqual(after['previous_reserved_microusd'], before['previous_reserved_microusd'])
        self.assertEqual(after['cap_microusd'], before['cap_microusd'])
        self.assertEqual(after['reservations'][0]['microusd'], 2 * CALL_HOLD)
        self.assertEqual(reconcile(request), after)
        acceptance_budget(self.request | {'run':str(uuid.uuid4())})
        with self.assertRaises(ValueError):
            acceptance_budget(self.request | {'run':str(uuid.uuid4())})

    def test_reconciliation_refuses_an_active_manager_lock(self):
        before = acceptance_budget(self.request)
        root = self.fixture_state()
        with (root / 'state/manager.lock').open('r+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaises(BlockingIOError):
                reconcile({'root':str(root), 'run':self.request['run'],
                           'ledger':self.request['path'], 'evidence_sha256':'fixture-proof'})
        self.assertEqual(acceptance_budget(self.request), before)

    def test_reconciliation_refuses_the_actual_manager_lock(self):
        binary = Path(os.environ.get('BOTSTRAL_MANAGER_BIN', str(Path(__file__).resolve().parents[2] / 'target/debug/botstral-manager'))).resolve()
        if not binary.is_file():
            self.skipTest('build botstral-manager or set BOTSTRAL_MANAGER_BIN')
        before = acceptance_budget(self.request)
        root = Path(self.directory.name) / self.request['run']
        root.mkdir(mode=0o700)
        state = root / 'state'
        child = subprocess.Popen([str(binary), '--state-dir', str(state), '--listen', '127.0.0.1:0', '--setup-listen', '127.0.0.1:0'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 10
            while not (state / 'listeners.json').exists():
                self.assertIsNone(child.poll(), 'manager exited before readiness')
                self.assertLess(time.monotonic(), deadline, 'manager readiness timeout')
                time.sleep(0.01)
            with self.assertRaises(BlockingIOError):
                reconcile({'root':str(root), 'run':self.request['run'], 'ledger':self.request['path'], 'evidence_sha256':'fixture-proof'})
            self.assertEqual(acceptance_budget(self.request), before)
        finally:
            child.terminate()
            self.assertEqual(child.wait(timeout=10), 0)


if __name__ == '__main__':
    unittest.main()
