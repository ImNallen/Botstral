from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import tempfile
import unittest
import uuid

from m04_remote import acceptance_budget
from m04_two_vm import CALL_HOLD, CALL_PLAN


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


if __name__ == '__main__':
    unittest.main()
