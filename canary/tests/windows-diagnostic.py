#!/usr/bin/env python3
"""Failure-injection contracts for the native Windows diagnostic collector."""
import importlib.util
import json
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
        return type("Args", (), {"source": "a" * 40, "mode": mode, "model": "test/model", "python_version": "3.12", "timeout": 1,
                                  "binary": Path("nanh.exe"), "canary": Path("nan-harness-canary.exe")})()

    def binaries(self, args, directory):
        args.binary = directory / "nanh.exe"; args.canary = directory / "nan-harness-canary.exe"
        args.binary.write_bytes(b"binary"); args.canary.write_bytes(b"canary")

    def test_budget_uses_monotonic_clock_and_reserves_cleanup(self):
        now = [100.0]
        budget = diagnostic.BatchBudget(40, lambda: now[0])
        self.assertEqual(budget.child_timeout(900), 20.0)
        now[0] = 141.0
        self.assertEqual(budget.child_timeout(900), 0.0)
        self.assertTrue(budget.exhausted())

    def test_workflow_budget_math_reserves_startup_and_finalization(self):
        self.assertEqual(diagnostic.diagnostic_batch_budget(8 * 60 + 36), 5964)
        self.assertEqual(diagnostic.diagnostic_batch_budget(110 * 60), 0 + 1)
        self.assertEqual(diagnostic.diagnostic_batch_budget(0, job_seconds=120, finalization_seconds=10,
                                                            startup_slack_seconds=5, cap_seconds=100), 100)

    def test_metadata_resolution_receives_remaining_bounded_timeout(self):
        args = self.args(); seen = []
        class TimedResolver:
            def resolve_manifest(self, harnesses, *_args, timeout=0):
                seen.append(timeout); return [Item(harnesses[0])], []
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=TimedResolver()), \
             patch.object(diagnostic.shutil, "which", return_value=None):
            args.budget_seconds = 50; args.clock = lambda: 0.0; self.binaries(args, Path(tmp))
            diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertEqual(seen, [20])

    def test_checkpoint_snapshot_is_pure_and_recomputes_active_outcome(self):
        args = self.args()
        reports = []
        active = {"harness": "codex", "phase": "deterministic-contract", "phases": {"metadata": diagnostic.phase("PASS"),
                                                    "prerequisites": diagnostic.phase("PASS"),
                                                    "install": diagnostic.phase("PASS"),
                                                    "version-doctor": diagnostic.phase("PASS"),
                                                    "deterministic-contract": diagnostic.phase("BLOCKED", "unfinished")}}
        snapshot = diagnostic._report(args, reports, {"status": "NOT_RUN"}, ["codex"], active=active)
        self.assertEqual(snapshot["harnesses"][0]["outcome"], "blocked")
        self.assertEqual(reports, [])
        active["phases"]["deterministic-contract"] = diagnostic.phase("PASS")
        snapshot = diagnostic._report(args, reports, {"status": "NOT_RUN"}, ["codex"], active=active)
        self.assertEqual(snapshot["harnesses"][0]["outcome"], "passed")

    def test_interruption_checkpoint_preserves_install_and_unfinished_doctor(self):
        args = self.args(); calls = [0]
        def interrupted(argv, cwd, env, timeout):
            calls[0] += 1
            if "-Stage" in argv and "version-doctor" in argv:
                raise KeyboardInterrupt
            if "-Stage" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed","diagnostics":[],"exitCode":0}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=interrupted):
            self.binaries(args, Path(tmp)); output = Path(tmp)
            with self.assertRaises(KeyboardInterrupt):
                diagnostic.collect(args, ["codex"], output)
            persisted = json.loads((output / "report.json").read_text())
        item = persisted["harnesses"][0]
        self.assertEqual(item["phases"]["install"]["status"], "PASS")
        self.assertEqual(item["phases"]["version-doctor"]["reason"], "unfinished")
        self.assertEqual(item["phases"]["deterministic-contract"]["reason"], "not-started")
        self.assertEqual(item["outcome"], "blocked")

    def test_deadline_checkpoint_preserves_partial_cells_and_explicit_not_started(self):
        args = self.args(); now = [0.0]
        def fake(argv, cwd, env, timeout):
            now[0] = 20.0
            return (1, "nonzero")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            args.budget_seconds = 30; args.clock = lambda: now[0]; self.binaries(args, Path(tmp))
            report, failed = diagnostic.collect(args, ["claude-code", "codex", "opencode"], Path(tmp))
            persisted = json.loads((Path(tmp) / "report.json").read_text())
        self.assertTrue(failed); self.assertEqual(report["totals"]["selected"], 3)
        self.assertEqual(persisted["harnesses"][0]["outcome"], "failed")
        self.assertEqual(persisted["harnesses"][2]["phases"]["install"]["reason"], "not-started")
        self.assertEqual(persisted["harnesses"][2]["outcome"], "blocked")
        self.assertEqual(persisted, report)

    def test_known_deadline_is_persisted_before_unattempted_metadata(self):
        args = self.args(); args.budget_seconds = 1; args.clock = lambda: 0.0
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
            persisted = json.loads((Path(tmp) / "report.json").read_text())
        self.assertTrue(failed); self.assertEqual(persisted, report)
        self.assertEqual(report["harnesses"][0]["phases"]["metadata"]["reason"], "deadline-exhausted")

    def test_cleanup_failure_is_explicit_and_does_not_publish_child_text(self):
        args = self.args()
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", return_value=(None, "timeout")), \
             patch.object(diagnostic, "cleanup_installer_artifacts", return_value=False):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        self.assertEqual(report["harnesses"][0]["phases"]["install"]["reason"], "installer-timeout")
        self.assertEqual(report["harnesses"][0]["phases"]["install"]["causeDetails"],
                         {"parentReason": "timeout", "installerReason": "installer-failed", "cleanupReason": "cleanup-failed"})

    def write_probe_success(self, argv, env):
        if "-Stage" in argv:
            Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                '{"schemaVersion":2,"stage":"complete","status":"passed",'
                '"diagnostics":[],"exitCode":0}')

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
            self.write_probe_success(argv, env)
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

    def test_native_self_test_reports_each_independent_boundary(self):
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), patch.object(diagnostic, "protect_private"), \
                 patch.object(diagnostic, "run_bounded", return_value=(1, "nonzero")), \
                 patch.object(diagnostic, "run_bounded_command_line", return_value=(1, "nonzero")):
                result = diagnostic.native_prerequisite_self_test(cell, {"ComSpec": r"C:\Windows\System32\cmd.exe"}, 1)
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(set(result["checks"]), {
            "pwsh-parser", "cmd-node-npm", "npm-registry", "npm-isolation", "python-venv", "python-pip", "git-bash", "job-dacl", "doctor-json", "cmd-argument-roundtrip"})
        self.assertTrue(all(value["status"] == "FAIL" for value in result["checks"].values()))

    def test_native_self_test_uses_cmd_argument_boundary_and_safe_reasons(self):
        calls = []
        def fake(argv, cwd, env, timeout):
            calls.append(argv)
            return (1, "nonzero")
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), \
                 patch.object(diagnostic, "protect_private"), patch.object(diagnostic.shutil, "which", return_value="resolved"), \
                 patch.object(diagnostic, "run_bounded", side_effect=fake), \
                 patch.object(diagnostic, "run_bounded_command_line", side_effect=fake):
                result = diagnostic.native_prerequisite_self_test(cell, {"ComSpec": r"C:\\Windows\\System32\\cmd.exe", "PATH": "safe"}, 1)
        cmd = next(argv for argv in calls if isinstance(argv, str) and "cmd.exe" in argv and "npm.cmd --version" in argv)
        self.assertIn(" /d /c ", cmd)
        self.assertIn('native self-test space', cmd)
        self.assertEqual(result["checks"]["cmd-node-npm"]["reason"], "version-probe-failed")
        self.assertEqual(result["checks"]["npm-registry"]["reason"], "registry-probe-failed")
        self.assertEqual(result["checks"]["cmd-argument-roundtrip"]["reason"], "argument-roundtrip-failed")
        self.assertIn('NAN_CMD_ARG_OK', next(argv for argv in calls if isinstance(argv, str) and "NAN_CMD_ARG_OK" in argv))

    def test_native_python_probe_selects_configured_minor(self):
        calls = []
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), patch.object(diagnostic, "protect_private"), \
                 patch.object(diagnostic.shutil, "which", return_value="resolved"), \
                 patch.object(diagnostic, "run_bounded", side_effect=lambda argv, *rest: (calls.append(argv) or (0, "exit"))), \
                 patch.object(diagnostic, "run_bounded_command_line", return_value=(0, "exit")):
                diagnostic.native_prerequisite_self_test(cell, {"ComSpec": r"C:\\Windows\\System32\\cmd.exe", "PATH": "safe"}, 1, "3.12")
        self.assertIn(["py.exe", "-3.12", "-m", "venv"], [argv[:4] for argv in calls if isinstance(argv, list)])

    def test_native_self_test_distinguishes_missing_path_tool(self):
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), \
                 patch.object(diagnostic, "protect_private"), patch.object(diagnostic.shutil, "which", return_value=None), \
                 patch.object(diagnostic, "run_bounded") as run, patch.object(diagnostic, "run_bounded_command_line") as raw_run:
                result = diagnostic.native_prerequisite_self_test(cell, {"ComSpec": r"C:\\Windows\\System32\\cmd.exe", "PATH": "safe"}, 1)
        self.assertEqual(result["checks"]["cmd-node-npm"]["reason"], "executable-missing")
        self.assertEqual(result["checks"]["git-bash"]["reason"], "git-for-windows-missing")
        self.assertEqual(result["checks"]["npm-registry"]["reason"], "executable-missing")
        self.assertNotEqual(run.call_count + raw_run.call_count, 0)

    def test_native_checkpoint_preserves_first_result_before_second_child_interrupts(self):
        snapshots = []
        calls = [0]
        def fake(*_args):
            calls[0] += 1
            if calls[0] == 2:
                raise KeyboardInterrupt
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp:
            cell = Path(tmp)
            with patch.object(diagnostic.os, "name", "nt"), patch.object(diagnostic, "protect_private"), \
                 patch.object(diagnostic.shutil, "which", return_value="resolved"), \
                 patch.object(diagnostic, "run_bounded", side_effect=fake), \
                 patch.object(diagnostic, "run_bounded_command_line", side_effect=fake):
                with self.assertRaises(KeyboardInterrupt):
                    diagnostic.native_prerequisite_self_test(
                        cell, {"ComSpec": r"C:\\Windows\\System32\\cmd.exe", "PATH": "safe"}, 1,
                        checkpoint=lambda name, checks, complete: snapshots.append((name, checks, complete)))
        self.assertEqual(snapshots[0][0], "pwsh-parser")
        self.assertFalse(snapshots[0][2]); self.assertEqual(snapshots[1][0], "pwsh-parser")
        self.assertEqual(snapshots[1][1]["pwsh-parser"]["status"], "PASS")
        self.assertEqual(snapshots[2][0], "cmd-node-npm")
        self.assertEqual(snapshots[2][1]["cmd-node-npm"]["reason"], "unfinished")

    def test_git_bash_resolves_only_git_for_windows_bundle(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); git = root / "cmd" / "git.exe"; bundled = root / "usr" / "bin" / "bash.exe"
            git.parent.mkdir(parents=True); bundled.parent.mkdir(parents=True); git.write_bytes(b""); bundled.write_bytes(b"")
            with patch.object(diagnostic.shutil, "which", return_value=str(git)):
                self.assertEqual(diagnostic._git_for_windows_bash({"PATH": "safe"}), str(bundled))

    def test_grouped_install_causes_are_shared_but_keep_harness_ids(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            (cwd / "installer-result.json").write_text(
                '{"schemaVersion":2,"status":"failed","reason":"installer-failed",'
                '"diagnostic":{"subphase":"install","executable":"npm-cmd",'
                '"exitCode":1,"npmCode":"registry-dns","processReason":"exit-nonzero"}}')
            return (1, "nonzero")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, _ = diagnostic.collect(args, ["codex", "qwen-code"], Path(tmp))
        groups = [value for value in report["groupedCauses"].values() if value["phase"] == "install"]
        self.assertEqual(len(groups), 1); self.assertEqual(groups[0]["count"], 2)
        self.assertEqual(groups[0]["harnesses"], ["codex", "qwen-code"])

    def test_live_without_credentials_is_blocked_and_required(self):
        args = self.args("live")
        def fake(argv, cwd, env, timeout):
            self.write_probe_success(argv, env); return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
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
            calls.append((argv, env)); self.write_probe_success(argv, env); return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "secret", "GITHUB_TOKEN": "secret"}):
            self.binaries(args, Path(tmp)); report, _ = diagnostic.collect(args, ["hermes"], Path(tmp))
        install = next(call for call in calls if "-Harness" in call[0] and "hermes" in call[0])
        self.assertEqual(install[0][0], "pwsh"); self.assertIn("-Ref", install[0]); self.assertIn("a" * 40, install[0])
        self.assertIn(("-PythonVersion", "3.12"), list(zip(install[0], install[0][1:])))
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
        self.assertEqual(install["reason"], "installer-nonzero")
        self.assertEqual(install["causeDetails"]["parentReason"], "nonzero")
        self.assertEqual(install["causeDetails"]["installerReason"], "capability-not-implemented")
        self.assertNotIn("raw", str(report))

    def test_v2_installer_diagnostic_reaches_phase_with_closed_values(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Harness" in argv and "codex" in argv:
                (cwd / "installer-result.json").write_text(
                    '{"schemaVersion":2,"status":"failed","reason":"installer-failed",'
                    '"diagnostic":{"subphase":"install","executable":"npm-cmd",'
                    '"exitCode":1,"npmCode":"registry-dns","processReason":"exit-nonzero"}}')
                return (1, "nonzero")
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        install = report["harnesses"][0]["phases"]["install"]
        self.assertEqual(install["reason"], "installer-nonzero")
        self.assertEqual(install["causeDetails"]["parentReason"], "nonzero")
        self.assertEqual(install["causeDetails"]["installerReason"], "installer-failed")
        self.assertEqual(install["diagnostic"], {"subphase": "install", "executable": "npm-cmd", "exitCode": 1,
                                                  "npmCode": "registry-dns", "processReason": "exit-nonzero"})

    def test_invalid_v2_installer_diagnostic_falls_back_without_raw_values(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            (cwd / "installer-result.json").write_text(
                '{"schemaVersion":2,"status":"failed","reason":"installer-failed",'
                '"diagnostic":{"subphase":"install","executable":"not-a-real-tool",'
                '"exitCode":1,"message":"secret"}}')
            return (1, "nonzero")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        install = report["harnesses"][0]["phases"]["install"]
        self.assertNotIn("diagnostic", install)
        self.assertNotIn("secret", str(report))

    def test_probe_v2_diagnostics_and_exit_code_are_closed_in_phase(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "version-doctor" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"version-doctor","status":"failed",'
                    '"diagnostics":["doctor-exit-nonzero","doctor-schema-invalid"],"exitCode":17,'
                    '"doctorSchemaReason":"field-type"}')
                return (1, "nonzero")
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        doctor = report["harnesses"][0]["phases"]["version-doctor"]
        self.assertEqual(doctor["reason"], "probe-nonzero")
        self.assertEqual(doctor["diagnostic"], {"stage": "version-doctor", "diagnostics": ["doctor-exit-nonzero", "doctor-schema-invalid"], "exitCode": 17, "doctorSchemaReason": "field-type"})

    def test_invalid_probe_diagnostics_do_not_escape_as_raw_data(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "version-doctor" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"version-doctor","status":"failed",'
                    '"diagnostics":["secret"],"exitCode":999999}')
                return (1, "nonzero")
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        doctor = report["harnesses"][0]["phases"]["version-doctor"]
        self.assertNotIn("diagnostic", doctor)
        self.assertNotIn("secret", str(report))

    def test_probe_v2_success_requires_complete_zero_exit_and_empty_diagnostics(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            self.write_probe_success(argv, env)
            if "-Stage" in argv and "version-doctor" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed",'
                    '"diagnostics":[],"exitCode":0}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertFalse(failed)
        self.assertEqual(report["harnesses"][0]["phases"]["version-doctor"]["status"], "PASS")

    def test_probe_v2_success_rejects_missing_exit_evidence(self):
        args = self.args()
        def fake(argv, cwd, env, timeout):
            if "-Stage" in argv and "version-doctor" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"complete","status":"passed",'
                    '"diagnostics":[]}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        self.assertEqual(report["harnesses"][0]["phases"]["version-doctor"]["status"], "FAIL")

    def test_probe_producer_examples_are_consumable_by_collector(self):
        producer = (ROOT / "guest" / "probe-harness.ps1").read_text(encoding="utf-8")
        self.assertIn("diagnostics = @($diagnostics.ToArray())", producer)
        self.assertIn("$value.exitCode = [int]$exitCode", producer)
        examples = (
            {"schemaVersion": 2, "stage": "complete", "status": "passed", "diagnostics": [], "exitCode": 0},
            {"schemaVersion": 2, "stage": "completion-marker", "status": "failed",
             "diagnostics": ["live-completion-marker-missing"]},
        )
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "probe-result.json"
            for example in examples:
                marker.write_text(json.dumps(example))
                parsed = diagnostic.probe_diagnostic(Path(tmp), "live-tool")
                self.assertEqual(parsed["status"], example["status"])

    def test_probe_v2_doctor_and_inventory_fields_are_closed(self):
        reasons = ("process-failed", "marker-missing", "provider-failed",
                   "provider-shutdown-failed", "daemon-cleanup-failed")
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "probe-result.json"
            for reason in reasons:
                marker.write_text(json.dumps({
                    "schemaVersion": 2, "stage": "deterministic-contract", "status": "failed",
                    "diagnostics": ["conformance-inventory-failed"], "exitCode": 1,
                    "inventoryFailureReasons": [reason]}))
                parsed = diagnostic.probe_diagnostic(Path(tmp), "deterministic-contract")
                self.assertEqual(parsed["inventoryFailureReasons"], [reason])
            marker.write_text(json.dumps({
                "schemaVersion": 2, "stage": "version-doctor", "status": "failed",
                "diagnostics": ["doctor-version-mismatch"], "exitCode": 1,
                "doctorVersion": "1.2.3", "doctorExpectedVersion": "1.2.4",
                "doctorReason": "mismatch"}))
            parsed = diagnostic.probe_diagnostic(Path(tmp), "version-doctor")
            self.assertEqual(parsed["doctorVersion"], "1.2.3")
            self.assertEqual(parsed["doctorReason"], "mismatch")

    def test_probe_v2_rejects_bad_optional_fields_and_unknown_inventory_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "probe-result.json"
            base = {"schemaVersion": 2, "stage": "version-doctor", "status": "failed",
                    "diagnostics": ["doctor-version-mismatch"], "exitCode": 1}
            for field, value in (("doctorVersion", "1"), ("doctorReason", "secret"),
                                 ("discoveryCode", "NH-DISCOVERY-999"),
                                 ("inventoryFailureReasons", ["raw"]),
                                 ("unknown", "value")):
                marker.write_text(json.dumps({**base, field: value}))
                self.assertEqual(diagnostic.probe_diagnostic(Path(tmp), "version-doctor")["status"], "failed")

    def test_live_failure_marker_may_omit_exit_when_no_subprocess_ran(self):
        args = self.args("live")
        def fake(argv, cwd, env, timeout):
            self.write_probe_success(argv, env)
            if "-Stage" in argv and "live-tool" in argv:
                Path(env["NAN_CANARY_PROBE_RESULT"]).write_text(
                    '{"schemaVersion":2,"stage":"completion-marker","status":"failed",'
                    '"diagnostics":["live-completion-marker-missing"]}')
            return (0, "exit")
        with tempfile.TemporaryDirectory() as tmp, patch.object(diagnostic, "resolver_module", return_value=Resolver()), \
             patch.object(diagnostic.shutil, "which", return_value="x"), patch.object(diagnostic, "run_bounded", side_effect=fake), \
             patch.dict(diagnostic.os.environ, {"NAN_API_KEY": "synthetic"}):
            self.binaries(args, Path(tmp)); report, failed = diagnostic.collect(args, ["codex"], Path(tmp))
        self.assertTrue(failed)
        live = report["harnesses"][0]["phases"]["live-tool"]
        self.assertEqual(live["status"], "FAIL")
        self.assertEqual(live["diagnostic"]["stage"], "completion-marker")


if __name__ == "__main__": unittest.main()
