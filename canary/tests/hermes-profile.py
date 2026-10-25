#!/usr/bin/env python3
"""Closed qualification receipts and the managed-profile file boundary."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "canary/actions"))
import cell

spec = importlib.util.spec_from_file_location("managed_hermes_profile", ROOT / "crates/nan-harness-runtime/src/prepared/hermes/nanh_profile.py")
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)
spec = importlib.util.spec_from_file_location("managed_hermes_main", ROOT / "crates/nan-harness-runtime/src/prepared/hermes/main.py")
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)
sys.path.insert(0, str(ROOT / "canary/hermes"))
spec = importlib.util.spec_from_file_location("native_hermes_qualification", ROOT / "canary/hermes/check.py")
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class Qualification(unittest.TestCase):
    def test_launch_wrapper_waits_for_the_child_and_preserves_its_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "child-completed"
            child = ("import os,sys,time; from pathlib import Path; time.sleep(0.1); "
                     "assert os.environ['NAN_API_KEY']==sys.argv[2]; "
                     "Path(sys.argv[1]).write_text('completed'); sys.exit(7)")
            result = subprocess.run([sys.executable, "-c", native.LAUNCH_WRAPPER,
                                     sys.executable, "-c", child, str(marker), native.KEY],
                                    capture_output=True, timeout=10, check=False)
            self.assertEqual(result.returncode, 7)
            self.assertEqual(marker.read_text(), "completed")

    def probe(self, value, status=0, failure=None):
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(directory=Path(directory), binary=Path("synthetic-nanh"))
            marker = args.directory / "hermes-profile-private.json"

            def command(*_args, **_kwargs):
                if failure:
                    raise failure
                marker.write_text(json.dumps(value))
                return status

            with patch.object(cell.shutil, "which", return_value="synthetic-hermes"), patch.object(cell, "private_command", side_effect=command):
                try:
                    cell.hermes_profile_probe(args, {"PATH": "synthetic"})
                finally:
                    self.assertFalse(marker.exists())

    def test_requires_the_complete_exact_positive_receipt(self):
        valid = {"schemaVersion": 1, "status": "passed", "checks": cell.HERMES_PROFILE_CHECKS}
        self.probe(valid)
        for value, status in [(dict(valid, checks=valid["checks"][:-1]), 0), (dict(valid, extra="synthetic"), 0),
                              (dict(valid, schemaVersion=True), 0), (valid, 1)]:
            with self.subTest(value=value, status=status), self.assertRaises(cell.ConformanceFailure):
                self.probe(value, status)

    def test_exposes_only_closed_failure_phases_and_prioritizes_cleanup(self):
        for phase, cleanup, expected in [("history-resume", "proven", "history-resume"),
                                         ("reasoning-effort", "unproven", "cleanup")]:
            with self.subTest(phase=phase), self.assertRaises(cell.ConformanceFailure) as caught:
                self.probe({"schemaVersion": 1, "status": "failed", "phase": phase, "cleanup": cleanup}, 1)
            self.assertEqual(caught.exception.code, "conformance-hermes-" + expected + "-exit-nonzero")
        with self.assertRaises(cell.ConformanceFailure) as caught:
            self.probe({"schemaVersion": 1, "status": "failed", "phase": "private synthetic payload"}, 1)
        self.assertEqual(caught.exception.code, "conformance-hermes-report-invalid")
        with self.assertRaises(cell.ConformanceFailure) as caught:
            self.probe({"schemaVersion": 1, "status": "failed", "phase": "history-create", "cleanup": "proven",
                        "diagnostics": ["NH-HERMES-READINESS", "private synthetic payload"]}, 1)
        self.assertEqual(caught.exception.code, "conformance-hermes-history-create-readiness-exit-nonzero")

    def test_missing_and_timed_out_probes_cannot_qualify(self):
        for failure, code in [(OSError(), "unavailable"), (cell.StageTimeout(), "timeout")]:
            with self.subTest(code=code), self.assertRaises(cell.ConformanceFailure) as caught:
                self.probe(None, failure=failure)
            self.assertEqual(caught.exception.code, "conformance-hermes-" + code)


class PrivateFiles(unittest.TestCase):
    def test_routing_cannot_be_overridden_or_bypassed_after_native_parsing(self):
        runtime.require_creation_routing(SimpleNamespace(reasoning="high"))
        for field, value in [("provider", "other"), ("model", "other"), ("api_key", "synthetic"),
                             ("base_url", "http://127.0.0.1:1"), ("safe_mode", True), ("ignore_user_config", True)]:
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                runtime.require_creation_routing(SimpleNamespace(**{field: value}))
    def test_update_requires_an_existing_regular_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.yaml"
            with self.assertRaises(RuntimeError):
                profile.private_write(path, "synthetic")
            path.write_text("old")
            path.chmod(0o600)
            profile.private_write(path, "new")
            self.assertEqual(path.read_text(), "new")
            if os.name != "nt":
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    @unittest.skipIf(os.name == "nt", "native Windows ACL coverage runs in the hosted cell")
    def test_update_never_follows_a_link_into_native_state(self):
        with tempfile.TemporaryDirectory() as directory:
            original = Path(directory) / "native"
            original.write_text("preserve")
            link = Path(directory) / "config.yaml"
            link.symlink_to(original)
            with self.assertRaises(RuntimeError):
                profile.private_write(link, "replace")
            self.assertEqual(original.read_text(), "preserve")


if __name__ == "__main__":
    unittest.main()
