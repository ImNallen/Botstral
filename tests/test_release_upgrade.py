import test_release as fixtures
from release_model import ReleaseIdentity
import unittest

class UpgradePolicyTests(unittest.TestCase):
    def test_downgrade_channel_and_rebuild_refusals(self):
        from release_upgrade import replacement_policy
        old = fixtures.ReleaseTests().identity(True)
        stable = fixtures.ReleaseTests().identity()
        replacement_policy(old, stable, switch_channel=True)
        with self.assertRaisesRegex(ValueError, 'channel switch'):
            replacement_policy(old, stable)
        with self.assertRaisesRegex(ValueError, 'downgrade'):
            replacement_policy(stable, old, switch_channel=True)
        newer = ReleaseIdentity.parse(dict(old.json(), version='0.1.0-nightly.20261031.1'))
        replacement_policy(old, newer)
        with self.assertRaisesRegex(ValueError, 'downgrade'):
            replacement_policy(newer, old)
        with self.assertRaisesRegex(ValueError, 'different identity'):
            replacement_policy(stable, ReleaseIdentity.parse(dict(stable.json(), build_id='different')))
