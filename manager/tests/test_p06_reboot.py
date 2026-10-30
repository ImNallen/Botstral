import argparse
import contextlib
import io
import json
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import p06_reboot


class RebootEvidenceTests(unittest.TestCase):
    def exercise(self, failure=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "manager"
            binary.write_bytes(b"synthetic binary; never executed")
            evidence = root / "evidence"
            args = argparse.Namespace(guest="botstral-manager", binary=binary,
                                      evidence=evidence, timeout=1)
            binary_hash = p06_reboot.digest(binary)
            calls = []
            exports = 0

            def command(argv, **_):
                nonlocal exports
                if argv[-1] == "sudo -n systemctl reboot":
                    return subprocess.CompletedProcess(argv, 0, "", "")
                if argv[-1] == "cat /proc/sys/kernel/random/boot_id":
                    return subprocess.CompletedProcess(argv, 0, "new-boot\n", "")
                arguments = shlex.split(argv[-1])
                operation = arguments[2]
                calls.append(operation)
                unit = "botstral-" + Path(arguments[4]).name + ".service"
                code = 0
                value = {}
                if operation == "export":
                    exports += 1
                    value = {
                        "before.json": json.dumps({"boot_id": "old-boot", "binary_sha256": binary_hash}),
                        "after.json": json.dumps({"boot_id": "new-boot", "binary_sha256": binary_hash}),
                        "inspection.json": "{}",
                        "manager.log": "",
                        "events.jsonl": json.dumps({"check": "reboot_verification_passed"}) + "\n",
                        unit: "[Service]\n",
                    }
                    if exports == 2:
                        if failure in ("export", "after_export"):
                            code = 1
                        elif failure == "missing_after":
                            del value["after.json"]
                        elif failure == "missing_verdict":
                            value["events.jsonl"] = ""
                        elif failure == "identity":
                            value["after.json"] = json.dumps({"boot_id": "old-boot", "binary_sha256": binary_hash})
                elif operation == "cleanup":
                    value = {"stopped": True, "unit_removed": True, "root_removed": True}
                    if failure == "cleanup":
                        code = 1
                elif operation == "after" and failure in ("after", "after_export"):
                    code = 1
                return subprocess.CompletedProcess(argv, code, json.dumps(value), "synthetic failure" if code else "")

            def local(argv, **_):
                return "synthetic-source" if argv[0] == "git" else ""

            error = None
            with patch.object(p06_reboot, "run", side_effect=local), \
                    patch.object(p06_reboot.subprocess, "run", side_effect=command), \
                    contextlib.redirect_stdout(io.StringIO()):
                try:
                    p06_reboot.orchestrate(args)
                except RuntimeError as caught:
                    error = str(caught)
            return error, json.loads((evidence / "resources.json").read_text()), calls

    def test_complete_evidence_and_cleanup_pass(self):
        error, ledger, calls = self.exercise()
        self.assertIsNone(error)
        self.assertTrue(ledger["passed"])
        self.assertIn("cleanup", calls)

    def test_final_export_failure_fails_and_still_cleans_up(self):
        error, ledger, calls = self.exercise("export")
        self.assertIsNotNone(error, "final evidence export failure must fail acceptance")
        self.assertFalse(ledger["passed"])
        self.assertIn("export_error", ledger)
        self.assertIn("cleanup", calls)

    def test_missing_post_reboot_record_fails(self):
        error, ledger, calls = self.exercise("missing_after")
        self.assertIsNotNone(error, "post-reboot evidence must be retained")
        self.assertFalse(ledger["passed"])
        self.assertIn("cleanup", calls)

    def test_missing_completed_verdict_fails(self):
        error, ledger, calls = self.exercise("missing_verdict")
        self.assertIsNotNone(error, "completed reboot verdict must be retained")
        self.assertFalse(ledger["passed"])
        self.assertIn("cleanup", calls)

    def test_cleanup_failure_fails_acceptance(self):
        error, ledger, _ = self.exercise("cleanup")
        self.assertIsNotNone(error)
        self.assertFalse(ledger["passed"])
        self.assertIn("cleanup_error", ledger)

    def test_unchanged_boot_in_exported_evidence_fails(self):
        error, ledger, _ = self.exercise("identity")
        self.assertIsNotNone(error)
        self.assertFalse(ledger["passed"])

    def test_original_failure_survives_finalization(self):
        error, ledger, calls = self.exercise("after")
        self.assertIn("remote after failed", error)
        self.assertFalse(ledger["passed"])
        self.assertIn("cleanup", calls)

    def test_original_failure_survives_export_failure(self):
        error, ledger, calls = self.exercise("after_export")
        self.assertIn("remote after failed", error)
        self.assertIn("remote after failed", ledger["failure"])
        self.assertIn("export_error", ledger)
        self.assertFalse(ledger["passed"])
        self.assertIn("cleanup", calls)


if __name__ == "__main__":
    unittest.main()
