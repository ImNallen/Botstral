import json
from pathlib import Path
import shutil
import tempfile
import unittest

from m02_record import assemble


class MainFixtureShutdownEvidence(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        source = Path(__file__).with_name("evidence")
        shutil.copytree(source / "raw", self.directory / "raw")
        self.commit = json.loads((source / "m02-two-vm.json").read_text())["application_commit"]
        self.shutdowns = sorted((self.directory / "raw").glob("*.service.shutdown.json"))
        self.assertEqual(len(self.shutdowns), 7)

    def rewrite(self, path, mutate):
        value = json.loads(path.read_text())
        mutate(value)
        path.write_text(json.dumps(value))

    def test_complete_saved_shutdown_records_pass(self):
        self.assertEqual(assemble(self.directory, self.commit)["two_vm"]["summary"]["status"], "passed")

    def test_missing_shutdown_record_fails(self):
        self.shutdowns[0].unlink()
        with self.assertRaises(FileNotFoundError):
            assemble(self.directory, self.commit)

    def test_failed_shutdown_records_fail(self):
        for path in self.shutdowns:
            def fail(value):
                value["clean"] = False
                value["states"][-1]["ExecMainStatus"] = "9"
            self.rewrite(path, fail)
        with self.assertRaises(AssertionError):
            assemble(self.directory, self.commit)

    def test_clean_flag_does_not_override_failed_terminal_state(self):
        self.rewrite(self.shutdowns[0], lambda value: value["states"][-1].update({"ExecMainStatus": "9"}))
        with self.assertRaises(AssertionError):
            assemble(self.directory, self.commit)

    def test_cleanup_error_fails(self):
        self.rewrite(self.shutdowns[0], lambda value: value["cleanup_errors"].append("stop:RuntimeError"))
        with self.assertRaises(AssertionError):
            assemble(self.directory, self.commit)

    def test_wrong_unit_identity_fails(self):
        self.rewrite(self.shutdowns[0], lambda value: value.update({"unit": "m02-other.service"}))
        with self.assertRaises(AssertionError):
            assemble(self.directory, self.commit)


if __name__ == "__main__":
    unittest.main()
