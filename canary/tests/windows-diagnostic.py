#!/usr/bin/env python3
"""Failure-injection contracts for the native Windows diagnostic collector."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("windows_diagnostic", ROOT / "actions" / "windows_diagnostic.py")
diagnostic = importlib.util.module_from_spec(spec); spec.loader.exec_module(diagnostic)


class Item:
    def __init__(self, harness): self.harness, self.version, self.ref = harness, "1.2.3", ("a" * 40 if harness == "hermes" else "")


class Resolver:
    def resolve_manifest(self, harnesses, *_args): return [Item(harnesses[0])], []


class WindowsDiagnosticTests(unittest.TestCase):
    def args(self, mode="deterministic"):
        return type("Args", (), {"source": "a" * 40, "mode": mode, "model": "test/model", "timeout": 1,
                                  "binary": Path("nanh.exe"), "canary": Path("nan-harness-canary.exe")})()

    def binaries(self, args, directory):
        args.binary = directory / "nanh.exe"; args.canary = directory / "nan-harness-canary.exe"
        args.binary.write_bytes(b"binary"); args.canary.write_bytes(b"canary")

    def test_all_fifteen_have_every_phase_exclusive_totals(self):
        args = self.args()
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", return_value=(None, "timeout")):
            self.binaries(args, Path(tmp))
            report, failed = diagnostic.collect(args, list(diagnostic.HARNESSES), Path(tmp))
        self.assertTrue(failed); self.assertEqual(report["totals"]["selected"], 15)
        self.assertEqual(report["totals"]["passed"] + report["totals"]["failed"] + report["totals"]["blocked"], 15)
        for item in report["harnesses"]:
            self.assertEqual(set(item["phases"]), set(diagnostic.PHASES)); self.assertEqual(item["outcome"], "failed")

    def test_first_middle_timeout_does_not_stop_later_harnesses(self):
        args = self.args("live"); calls = []
        def fake(argv, cwd, env, timeout):
            calls.append((cwd.name, argv))
            return (None, "timeout") if cwd.name == "codex" else (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "synthetic"}):
            self.binaries(args, Path(tmp))
            report, failed = diagnostic.collect(args, ["claude-code", "codex", "opencode"], Path(tmp))
        self.assertTrue(failed); self.assertEqual([x["harness"] for x in report["harnesses"]], ["claude-code", "codex", "opencode"])
        self.assertEqual(report["harnesses"][2]["outcome"], "passed")
        self.assertTrue(any("deterministic-contract" in call[1] for call in calls))
        self.assertTrue(any("live-tool" in call[1] for call in calls))
        self.assertTrue(any(any("nanh.exe" in str(arg) for arg in call[1]) and
                            any("nan-harness-canary.exe" in str(arg) for arg in call[1]) for call in calls))

    def test_environment_strips_credentials_and_isolates_npm(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "secret", "GITHUB_TOKEN": "secret"}):
            env = diagnostic.isolated_environment(Path(tmp))
        self.assertNotIn("NAN_API_KEY", env); self.assertNotIn("GITHUB_TOKEN", env)
        self.assertIn("NPM_CONFIG_PREFIX", env); self.assertIn(str(Path(tmp) / "bin"), env["PATH"])

    def test_live_without_credentials_is_blocked_and_required(self):
        args = self.args("live")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", return_value=(0, "exit")), \
             patch.dict(diagnostic.os.environ, {}, clear=True):
            self.binaries(args, Path(tmp))
            report, failed = diagnostic.collect(args, ["claude-code"], Path(tmp))
        self.assertTrue(failed); self.assertEqual(report["harnesses"][0]["phases"]["live-tool"]["status"], "BLOCKED")

    def test_missing_build_still_reports_all_ids_and_blocks_nan_phases(self):
        args = self.args()
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"):
            report, failed = diagnostic.collect(args, list(diagnostic.HARNESSES), Path(tmp))
        self.assertTrue(failed); self.assertEqual(len(report["harnesses"]), 15)
        self.assertTrue(all(item["phases"]["version-doctor"]["status"] == "BLOCKED" for item in report["harnesses"]))

    def test_output_redacts_child_text_and_groups_causal_ids(self):
        args = self.args()
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value=None):
            report, _ = diagnostic.collect(args, ["claude-code"], Path(tmp)); diagnostic.write_outputs(report, Path(tmp))
            payload = (Path(tmp) / "report.json").read_text()
        self.assertNotIn("secret", payload.lower()); self.assertTrue(report["groupedCauses"])

    def test_install_argv_keeps_frozen_hermes_ref_and_private_tool_paths(self):
        args = self.args(); calls = []
        def fake(argv, cwd, env, timeout):
            calls.append((argv, env)); return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "secret", "GITHUB_TOKEN": "secret"}):
            self.binaries(args, Path(tmp)); report, _ = diagnostic.collect(args, ["hermes"], Path(tmp))
        install = calls[0]
        self.assertEqual(install[0][0], "pwsh"); self.assertIn("-Ref", install[0]); self.assertIn("a" * 40, install[0])
        self.assertNotIn("NAN_API_KEY", install[1]); self.assertNotIn("GITHUB_TOKEN", install[1])
        self.assertIn(str(Path(tmp) / "cells/hermes/hermes/bin"), install[1]["PATH"])
        self.assertIn(str(Path(tmp) / "cells/hermes/home/.nan-harness-canary-venv/Scripts"), install[1]["PATH"])

    def test_closed_installer_reason_reaches_phase_without_raw_message(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Harness" in argv and "hermes" in argv:
                (cwd / "installer-result.json").write_text(
                    '{"schemaVersion":1,"status":"failed","reason":"capability-not-implemented"}')
                return (1, "nonzero")
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["hermes"], Path(tmp))
        self.assertTrue(failed); install = report["harnesses"][0]["phases"]["install"]
        self.assertEqual(install["reason"], "installer-capability-not-implemented")
        self.assertNotIn("raw", str(report))


if __name__ == "__main__": unittest.main()
