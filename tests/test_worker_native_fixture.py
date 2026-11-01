import importlib.util
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from release_model import ROOT, authority


def load_container_acceptance():
    spec = importlib.util.spec_from_file_location(
        'worker_container_acceptance', ROOT / 'agent/packaging/container-acceptance.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class WorkerNativeFixtureTests(unittest.TestCase):
    def test_generated_check_config_uses_pin_not_manager_ca(self):
        fixture = load_container_acceptance().FIXTURE
        self.assertIn("'pin':", fixture)
        self.assertNotIn('manager_ca', fixture)
        linux = (ROOT / 'scripts/release_acceptance_linux.py').read_text()
        probe = linux.split('def worker_tls_probe', 1)[1].split('def accept', 1)[0]
        self.assertIn("'pin':", probe)
        self.assertNotIn('manager_ca', probe)

    def test_expected_user_version_comes_from_release_authority(self):
        schema = authority()['data_compatibility']['worker_schema']
        module = load_container_acceptance()
        self.assertIn('== WORKER_SCHEMA', module.FIXTURE)
        linux = (ROOT / 'scripts/release_acceptance_linux.py').read_text()
        self.assertIn("authority()['data_compatibility']['worker_schema']", linux)
        self.assertIn("['data_compatibility']['worker_schema']",
                      (ROOT / 'agent/packaging/container-acceptance.py').read_text())
        source = 'WORKER_SCHEMA = ' + str(schema) + '\n' + module.FIXTURE
        self.assertIn('WORKER_SCHEMA = ' + str(schema), source)


if __name__ == '__main__':
    unittest.main()
