#!/usr/bin/env python3
"""Execute isolated child environments and closed failed-report contracts."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import cell
import hosted


class CellIsolationTests(unittest.TestCase):
    def test_zcode_install_failure_exports_only_known_marker_categories(self):
        cases = [({"stage": "dependencies", "reason": "rename-missing"},
                  "zcode-source-dependencies-rename-missing"),
                 ({"stage": "dependencies", "reason": "private-secret"},
                  "zcode-source-dependencies"),
                 ({"stage": "build", "reason": "rename-missing"}, "zcode-source-build"),
                 ({"stage": "private-secret"}, "unknown"), ([], "unknown")]
        for details, expected in cases:
            with self.subTest(details=details), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                marker = root / "zcode-source-failure.json"
                marker.write_text(json.dumps(details))
                args = argparse.Namespace(directory=root, harness="zcode", harness_version="0.16.9")
                with patch.object(cell, "installer_command", return_value=["synthetic-installer"]), \
                        patch.object(cell, "private_command", return_value=1), \
                        patch.object(cell, "windows_install_failure", return_value="unknown"):
                    with self.assertRaises(cell.InstallFailure) as caught:
                        cell.install(args, {})
                self.assertEqual(caught.exception.code, expected)
                self.assertNotIn("private-secret", str(caught.exception))
                self.assertFalse(marker.exists())

    def test_harness_cells_cannot_inherit_each_others_installation_or_configuration(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shared = root / "shared-user"
            shared.mkdir()
            first, second = root / "first", root / "second"
            with patch.dict(os.environ, {"HOME": str(shared), "USERPROFILE": str(shared),
                                         "PATH": str(shared / ".local/bin") + os.pathsep + os.environ["PATH"]}):
                one, two = cell.cell_environment(first), cell.cell_environment(second)
            for key in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "HERMES_HOME",
                        "NPM_CONFIG_PREFIX", "UV_TOOL_DIR", "NAN_HARNESS_CONFIG_DIR", "TMPDIR"):
                self.assertTrue(Path(one[key]).is_relative_to(first), key)
                self.assertTrue(Path(two[key]).is_relative_to(second), key)
                self.assertNotEqual(one[key], two[key])
            self.assertNotIn(str(shared / ".local/bin"), one["PATH"].split(os.pathsep))
            Path(one["HOME"], "sentinel").write_text("first harness only")
            script = "import os,pathlib; assert not pathlib.Path(os.environ['HOME'],'sentinel').exists()"
            cell.private_command([sys.executable, "-c", script], second, environment=two)
            self.assertTrue(Path(one["HOME"], "sentinel").is_file())
            self.assertEqual(list(shared.iterdir()), [])

    def test_isolated_setup_still_removes_provider_credentials(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            environment = cell.cell_environment(root)
            environment["NAN_API_KEY"] = "synthetic-private-key"
            cell.private_command([sys.executable, "-c",
                                  "import os; assert 'NAN_API_KEY' not in os.environ"],
                                 root, environment=environment)
            self.assertEqual(environment["NAN_API_KEY"], "synthetic-private-key")

    def test_failed_live_report_retains_the_selected_model_as_retryable_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "binary"
            binary.write_bytes(b"synthetic binary")
            args = argparse.Namespace(directory=root, output=root / "report.json", canary=binary,
                                      binary=binary, harness="codex", model="selected-model",
                                      trigger="manual", stage="live")
            state = {"schemaVersion": 2, "nanHarness": {"version": "1.2.3", "sha256": cell.digest(binary)},
                     "harness": {"id": "codex", "version": "1.0.0"},
                     "environment": {"operatingSystem": "linux", "architecture": "aarch64"},
                     "startedAt": cell.timestamp(), "durationMilliseconds": 0,
                     "checks": [{"name": name, "status": "passed"} for name in
                                ("install-and-diagnose", "deterministic-conformance")]}
            cell.write_json(root / "state.json", state)
            with patch.object(cell, "private_command"):
                cell.failed_report(args)
            report = json.loads(args.output.read_bytes())
            self.assertEqual(report["model"], "selected-model")
            self.assertEqual(report["failure"]["class"], "infrastructure")
            update = hosted.report_update("cli", args.output.read_bytes(), "a" * 64, 123)
            self.assertEqual(update["hostedChecks"][-1]["model"], "selected-model")
            self.assertEqual(update["hostedChecks"][-1]["outcome"], "blocked")
            feed = {"schemaVersion": 5, "releases": [update]}
            at = hosted.instant(report["completedAt"])
            self.assertFalse(hosted.should_probe(feed, "1.2.3", update["hostedChecks"][-1], at))
            self.assertTrue(hosted.should_probe(feed, "1.2.3", update["hostedChecks"][-1],
                                              at + hosted.datetime.timedelta(days=1)))


if __name__ == "__main__":
    unittest.main()
