from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
import w01_remote


class CleanupTests(unittest.TestCase):
    def test_cleanup_reports_an_unavailable_service_manager(self):
        with tempfile.TemporaryDirectory() as directory:
            unit = "m02-" + Path(directory).name + "-production-daemon.service"
            failed = subprocess.CompletedProcess([], 1, "", "Failed to connect to bus")
            with patch.object(w01_remote.subprocess, "run", return_value=failed):
                with self.assertRaises(RuntimeError):
                    w01_remote.dispatch({"op": "cleanup_processes", "root": directory, "units": [unit]})

    def test_cleanup_accepts_an_explicitly_absent_unit(self):
        absent = subprocess.CompletedProcess([], 0, "LoadState=not-found\nActiveState=inactive\n", "")
        with patch.object(w01_remote.subprocess, "run", return_value=absent):
            result = w01_remote.stop_fixture_unit("m02-fixture-production-daemon.service")
        self.assertEqual(result["scope"], "absent")


if __name__ == "__main__":
    unittest.main()
