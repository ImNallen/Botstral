import contextlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import m02_two_vm as runner


class RunnerCompleteness(unittest.TestCase):
    def exercise(self, production):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            certs = root / "certs"
            certs.mkdir()
            for name in ("ca.pem", "manager.pem", "manager.key", "worker.pem", "worker.key",
                         "other.pem", "other.key", "wrong-ca.pem", "wrong-client.pem", "wrong-client.key",
                         "expired.pem", "expired.key"):
                (certs / name).write_text("synthetic certificate fixture")
            arguments = ["m02_two_vm.py", "--cert-dir", str(certs), "--output-dir", str(root / "results")]
            runner.RESULTS.clear()
            with patch("sys.argv", arguments), patch.object(runner, "setup"), \
                 patch.object(runner.Fixture, "finish"), \
                 patch.object(runner, "production", side_effect=production), \
                 patch.object(runner, "test_sessions"), contextlib.redirect_stdout(io.StringIO()):
                status = runner.main()
            return status

    def test_unexpected_error_between_checks_fails(self):
        self.assertNotEqual(self.exercise(RuntimeError("synthetic failure")), 0)

    def test_missing_required_checks_fail(self):
        self.assertNotEqual(self.exercise(None), 0)


if __name__ == "__main__":
    unittest.main()
