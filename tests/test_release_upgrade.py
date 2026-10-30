import test_release as fixtures
from release_model import MARKER, ReleaseIdentity, ReleaseManifest, write_json
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

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


class RepairPolicyTests(unittest.TestCase):
    def prepare_repair(self, root, phase='prepared', different_path=False, current=None):
        from release_upgrade import prepare
        directory = root / 'release'
        directory.mkdir()
        value = fixtures.ReleaseTests().manifest(directory)
        release = ReleaseManifest.parse(value)
        app = root / 'Botstral.app'
        installed = app / 'Contents/MacOS/botstral-desktop'
        if current:
            installed.parent.mkdir(parents=True)
            installed.write_bytes(MARKER + json.dumps({'identity': current.json()}).encode() + b'\0')
        artifact = next(a for a in release.artifacts if a.component == 'desktop')
        repair = root / 'repair.json'
        write_json(repair, {'phase': phase, 'installation_path': str(root / 'other' if different_path else installed),
                           'component': 'desktop', 'target': release.identity.json(), 'previous': None,
                           'artifact_sha256': artifact.sha256})
        backup = root / 'backup.json'
        write_json(backup, {'stopped_at': 'fixture', 'manager': {}, 'worker': {}})
        config = root / 'configuration'
        config.mkdir()
        args = SimpleNamespace(directory=directory, trusted_key=None, unsigned=True,
                               version=release.identity.version, component='desktop', app_path=app,
                               repair_receipt=repair, switch_channel=True, backup_receipt=backup,
                               config_backup=config, receipt=root / 'prepared.json', fresh_install=False)
        with patch('release_upgrade.inspect_artifacts'), patch('release_upgrade.stopped'), \
                patch('release_upgrade.sys.platform', 'darwin'), patch('release_upgrade.platform.machine', return_value='arm64'):
            prepare(args)
        return json.loads(args.receipt.read_text())

    def test_applied_receipt_cannot_authorize_repair(self):
        with tempfile.TemporaryDirectory() as directory, self.assertRaisesRegex(ValueError, 'incomplete prepared'):
            self.prepare_repair(Path(directory), phase='applied')

    def test_receipt_cannot_repair_another_destination(self):
        with tempfile.TemporaryDirectory() as directory, self.assertRaisesRegex(ValueError, 'different installation'):
            self.prepare_repair(Path(directory), different_path=True)

    def test_stale_prepared_receipt_cannot_downgrade_intact_installation(self):
        current = ReleaseIdentity.parse(dict(fixtures.ReleaseTests().identity().json(), version='0.2.0'))
        with tempfile.TemporaryDirectory() as directory, self.assertRaisesRegex(ValueError, 'unsafe downgrade'):
            self.prepare_repair(Path(directory), current=current)

    def test_incomplete_replacement_can_prepare_same_target_repair(self):
        with tempfile.TemporaryDirectory() as directory:
            receipt = self.prepare_repair(Path(directory))
            self.assertEqual(receipt['phase'], 'prepared')
            self.assertTrue(receipt['installation_path'].endswith('/Botstral.app/Contents/MacOS/botstral-desktop'))
